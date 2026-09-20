use crate::{ScriptApiVersion, ScriptId, ScriptLanguage, ScriptSource};
use engine_platform::{AtomicFileService, AtomicWriteError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use thiserror::Error;

pub const CURRENT_SCRIPT_MANIFEST_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptManifestEntry {
    pub id: ScriptId,
    pub language: ScriptLanguage,
    pub relative_path: PathBuf,
    pub api_version: ScriptApiVersion,
    #[serde(default)]
    pub public_properties: Vec<crate::PublicProperty>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScriptManifest {
    pub format_version: u32,
    pub scripts: Vec<ScriptManifestEntry>,
}

impl Default for ScriptManifest {
    fn default() -> Self {
        Self {
            format_version: CURRENT_SCRIPT_MANIFEST_VERSION,
            scripts: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ManifestLoad {
    pub manifest: ScriptManifest,
    pub migrated: bool,
    pub read_only: bool,
    pub quarantined_duplicates: Vec<ScriptId>,
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("script manifest is not valid UTF-8/RON: {0}")]
    Parse(String),
    #[error("script manifest version {found} is newer than supported {supported}")]
    FutureVersion { found: u32, supported: u32 },
    #[error("unsafe script path: {0}")]
    UnsafePath(PathBuf),
    #[error("script manifest serialization failed: {0}")]
    Serialize(String),
    #[error("script manifest write failed: {0}")]
    Write(#[from] AtomicWriteError),
    #[error("script asset I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Registers a source file and returns its persistent identity. Editor import
/// flows call this automatically; developers never author registry metadata.
///
/// # Errors
/// Returns an I/O, unsafe-path, parse, or registry persistence error.
pub fn register_script_asset(
    root: &Path,
    relative_path: impl Into<PathBuf>,
) -> Result<ScriptId, ManifestError> {
    let relative_path = relative_path.into();
    if !safe_relative(&relative_path) {
        return Err(ManifestError::UnsafePath(relative_path));
    }
    let language = ScriptLanguage::from_path(&relative_path)
        .ok_or_else(|| ManifestError::UnsafePath(relative_path.clone()))?;
    let source_path = root.join(&relative_path);
    if !source_path.is_file() {
        return Err(ManifestError::Io {
            path: source_path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "script source not found"),
        });
    }
    let manifest_path = root.join("config/scripts.ron");
    let mut manifest = if manifest_path.is_file() {
        load_manifest(
            &std::fs::read(&manifest_path).map_err(|source| ManifestError::Io {
                path: manifest_path.clone(),
                source,
            })?,
        )?
        .manifest
    } else {
        ScriptManifest::default()
    };
    if let Some(entry) = manifest
        .scripts
        .iter()
        .find(|entry| entry.relative_path == relative_path)
    {
        return Ok(entry.id);
    }
    let id = ScriptId::new();
    manifest.scripts.push(ScriptManifestEntry {
        id,
        language,
        relative_path,
        api_version: ScriptApiVersion::CURRENT,
        public_properties: Vec::new(),
    });
    save_manifest_atomic(&manifest_path, &manifest)?;
    Ok(id)
}

/// Moves or renames a registered script while retaining its identity. If the
/// registry update fails, the source move is rolled back.
///
/// # Errors
/// Returns an I/O, unsafe-path, unknown-asset, or registry persistence error.
pub fn move_script_asset(
    root: &Path,
    asset_id: ScriptId,
    new_relative_path: impl Into<PathBuf>,
) -> Result<(), ManifestError> {
    let new_relative_path = new_relative_path.into();
    if !safe_relative(&new_relative_path) {
        return Err(ManifestError::UnsafePath(new_relative_path));
    }
    let manifest_path = root.join("config/scripts.ron");
    let mut manifest =
        load_manifest(
            &std::fs::read(&manifest_path).map_err(|source| ManifestError::Io {
                path: manifest_path.clone(),
                source,
            })?,
        )?
        .manifest;
    let entry = manifest
        .scripts
        .iter_mut()
        .find(|entry| entry.id == asset_id)
        .ok_or_else(|| ManifestError::Parse(format!("unknown script asset {asset_id}")))?;
    let language = ScriptLanguage::from_path(&new_relative_path)
        .ok_or_else(|| ManifestError::UnsafePath(new_relative_path.clone()))?;
    let old_relative_path = entry.relative_path.clone();
    let old_path = root.join(&old_relative_path);
    let new_path = root.join(&new_relative_path);
    if let Some(parent) = new_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| ManifestError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }
    std::fs::rename(&old_path, &new_path).map_err(|source| ManifestError::Io {
        path: old_path.clone(),
        source,
    })?;
    entry.relative_path = new_relative_path;
    entry.language = language;
    if let Err(error) = save_manifest_atomic(&manifest_path, &manifest) {
        let _ = std::fs::rename(&new_path, &old_path);
        return Err(error);
    }
    Ok(())
}

#[derive(Deserialize)]
struct Probe {
    format_version: u32,
}
#[derive(Deserialize)]
struct ManifestV1 {
    scripts: Vec<EntryV1>,
}
#[derive(Deserialize)]
struct EntryV1 {
    id: ScriptId,
    relative_path: PathBuf,
}

/// Loads, validates, migrates, and quarantines duplicate identities.
///
/// # Errors
/// Returns an error for malformed, future-version, or unsafe-path data.
pub fn load_manifest(bytes: &[u8]) -> Result<ManifestLoad, ManifestError> {
    let text = std::str::from_utf8(bytes).map_err(|e| ManifestError::Parse(e.to_string()))?;
    let probe: Probe = ron::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;
    if probe.format_version > CURRENT_SCRIPT_MANIFEST_VERSION {
        return Err(ManifestError::FutureVersion {
            found: probe.format_version,
            supported: CURRENT_SCRIPT_MANIFEST_VERSION,
        });
    }
    let migrated = probe.format_version == 1;
    let mut manifest = if migrated {
        let old: ManifestV1 =
            ron::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?;
        ScriptManifest {
            format_version: CURRENT_SCRIPT_MANIFEST_VERSION,
            scripts: old
                .scripts
                .into_iter()
                .map(|e| ScriptManifestEntry {
                    id: e.id,
                    language: ScriptLanguage::Lua54,
                    relative_path: e.relative_path,
                    api_version: ScriptApiVersion::CURRENT,
                    public_properties: Vec::new(),
                })
                .collect(),
        }
    } else {
        ron::from_str(text).map_err(|e| ManifestError::Parse(e.to_string()))?
    };
    if let Some(entry) = manifest.scripts.iter().find(|entry| {
        !safe_relative(&entry.relative_path)
            || !entry.language.accepts_project_path(&entry.relative_path)
    }) {
        return Err(ManifestError::UnsafePath(entry.relative_path.clone()));
    }
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut duplicates = Vec::new();
    manifest.scripts.retain(|entry| {
        let unique = ids.insert(entry.id) && paths.insert(entry.relative_path.clone());
        if !unique {
            duplicates.push(entry.id);
        }
        unique
    });
    Ok(ManifestLoad {
        manifest,
        migrated,
        read_only: false,
        quarantined_duplicates: duplicates,
    })
}

/// Saves a deterministic manifest transactionally.
///
/// # Errors
/// Returns an error for unsafe paths, serialization, or transactional I/O failure.
pub fn save_manifest_atomic(path: &Path, manifest: &ScriptManifest) -> Result<(), ManifestError> {
    for entry in &manifest.scripts {
        if !safe_relative(&entry.relative_path)
            || !entry.language.accepts_project_path(&entry.relative_path)
        {
            return Err(ManifestError::UnsafePath(entry.relative_path.clone()));
        }
    }
    let mut normalized = manifest.clone();
    normalized.format_version = CURRENT_SCRIPT_MANIFEST_VERSION;
    normalized.scripts.sort_by_key(|e| e.id);
    let mut text =
        ron::ser::to_string_pretty(&normalized, ron::ser::PrettyConfig::new().depth_limit(8))
            .map_err(|e| ManifestError::Serialize(e.to_string()))?;
    text.push('\n');
    AtomicFileService::new().write(path, text.as_bytes())?;
    Ok(())
}

/// Hashes bounded UTF-8 source into its immutable descriptor.
///
/// # Errors
/// Returns an error for unsafe/missing/oversized/non-UTF-8 source.
pub fn source_descriptor(
    root: &Path,
    entry: &ScriptManifestEntry,
    maximum_bytes: u64,
) -> Result<ScriptSource, ManifestError> {
    if !safe_relative(&entry.relative_path) {
        return Err(ManifestError::UnsafePath(entry.relative_path.clone()));
    }
    let path = root.join(&entry.relative_path);
    let bytes = std::fs::read(&path).map_err(|e| ManifestError::Parse(e.to_string()))?;
    if bytes.len() as u64 > maximum_bytes {
        return Err(ManifestError::Parse(format!(
            "script exceeds {maximum_bytes} byte limit"
        )));
    }
    std::str::from_utf8(&bytes).map_err(|e| ManifestError::Parse(e.to_string()))?;
    Ok(ScriptSource {
        format_version: 1,
        id: entry.id,
        language: entry.language,
        relative_path: entry.relative_path.clone(),
        api_version: entry.api_version,
        content_hash: format!("{:x}", Sha256::digest(&bytes)),
    })
}

fn safe_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path.components().all(|c| matches!(c, Component::Normal(_)))
        && ScriptLanguage::from_path(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manifest_round_trip_migration_future_rejection_and_stable_move() {
        let id = ScriptId::new();
        let old =
            format!("(format_version:1,scripts:[(id:\"{id}\",relative_path:\"scripts/old.lua\")])");
        let loaded = load_manifest(old.as_bytes()).unwrap();
        assert!(loaded.migrated);
        assert_eq!(loaded.manifest.scripts[0].id, id);
        let mut moved = loaded.manifest;
        moved.scripts[0].relative_path = "scripts/new.lua".into();
        assert_eq!(moved.scripts[0].id, id);
        let future = b"(format_version:999,scripts:[])";
        assert!(matches!(
            load_manifest(future),
            Err(ManifestError::FutureVersion { .. })
        ));
    }
    #[test]
    fn duplicates_are_quarantined_and_traversal_is_rejected_on_save() {
        let id = ScriptId::new();
        let entry = ScriptManifestEntry {
            id,
            language: ScriptLanguage::Lua54,
            relative_path: "scripts/a.lua".into(),
            api_version: ScriptApiVersion::CURRENT,
            public_properties: Vec::new(),
        };
        let bytes = ron::to_string(&ScriptManifest {
            format_version: 2,
            scripts: vec![entry.clone(), entry],
        })
        .unwrap();
        let load = load_manifest(bytes.as_bytes()).unwrap();
        assert_eq!(load.manifest.scripts.len(), 1);
        assert_eq!(load.quarantined_duplicates, vec![id]);
        let temp = tempfile::tempdir().unwrap();
        let bad = ScriptManifest {
            format_version: 2,
            scripts: vec![ScriptManifestEntry {
                relative_path: "../evil.lua".into(),
                id,
                ..load.manifest.scripts[0].clone()
            }],
        };
        assert!(matches!(
            save_manifest_atomic(&temp.path().join("m.ron"), &bad),
            Err(ManifestError::UnsafePath(_))
        ));
    }

    #[test]
    fn web_and_php_manifest_entries_must_live_below_ui() {
        let make = |language, relative_path: &str| ScriptManifest {
            format_version: CURRENT_SCRIPT_MANIFEST_VERSION,
            scripts: vec![ScriptManifestEntry {
                id: ScriptId::new(),
                language,
                relative_path: relative_path.into(),
                api_version: ScriptApiVersion::CURRENT,
                public_properties: Vec::new(),
            }],
        };
        let temp = tempfile::tempdir().unwrap();
        assert!(
            save_manifest_atomic(
                &temp.path().join("web.ron"),
                &make(ScriptLanguage::Web, "ui/hud.html")
            )
            .is_ok()
        );
        assert!(matches!(
            save_manifest_atomic(
                &temp.path().join("bad-web.ron"),
                &make(ScriptLanguage::Web, "scripts/hud.html")
            ),
            Err(ManifestError::UnsafePath(_))
        ));
        assert!(matches!(
            save_manifest_atomic(
                &temp.path().join("bad-php.ron"),
                &make(ScriptLanguage::Php, "scripts/menu.php")
            ),
            Err(ManifestError::UnsafePath(_))
        ));
    }

    #[test]
    fn web_entry_below_scripts_is_rejected_on_load_and_save() {
        let manifest = ScriptManifest {
            format_version: CURRENT_SCRIPT_MANIFEST_VERSION,
            scripts: vec![ScriptManifestEntry {
                id: ScriptId::new(),
                language: ScriptLanguage::Web,
                relative_path: "scripts/legacy.html".into(),
                api_version: ScriptApiVersion::CURRENT,
                public_properties: Vec::new(),
            }],
        };
        let bytes = ron::to_string(&manifest).unwrap();
        assert!(matches!(
            load_manifest(bytes.as_bytes()),
            Err(ManifestError::UnsafePath(_))
        ));
        let temp = tempfile::tempdir().unwrap();
        assert!(matches!(
            save_manifest_atomic(&temp.path().join("legacy.ron"), &manifest),
            Err(ManifestError::UnsafePath(_))
        ));
    }
}
