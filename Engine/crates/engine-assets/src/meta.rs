//! Authoritative adjacent `.rmeta` sidecars.

use crate::{AssetError, io_error};
use engine_core::AssetId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

/// Current sidecar schema version.
pub const RMETA_FORMAT_VERSION: u32 = 1;
/// Upper bound for a sidecar to prevent memory exhaustion while scanning.
pub const MAX_RMETA_BYTES: u64 = 1024 * 1024;

/// Versioned import settings represented as deterministic string values.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ImportSettings {
    /// Settings schema version owned by the importer.
    pub version: u32,
    /// Deterministically ordered settings.
    #[serde(default)]
    pub values: BTreeMap<String, String>,
}

/// Authoritative metadata adjacent to one source asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetMeta {
    /// Sidecar schema version.
    pub format_version: u32,
    /// Stable source asset identity.
    pub asset_id: AssetId,
    /// Stable importer implementation identifier.
    pub importer_id: String,
    /// Importer behavior/schema version.
    pub importer_version: u32,
    /// Versioned deterministic settings.
    #[serde(default)]
    pub settings: ImportSettings,
    /// Last successfully imported source SHA-256, hexadecimal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    /// Stable asset dependencies, always serialized in ID order.
    #[serde(default)]
    pub dependencies: Vec<AssetId>,
}

impl AssetMeta {
    /// Creates a fresh sidecar for a source and importer contract.
    pub fn new(importer_id: impl Into<String>, importer_version: u32) -> Self {
        Self {
            format_version: RMETA_FORMAT_VERSION,
            asset_id: AssetId::new(),
            importer_id: importer_id.into(),
            importer_version,
            settings: ImportSettings::default(),
            source_hash: None,
            dependencies: Vec::new(),
        }
    }

    /// Validates schema and canonicalizes deterministic collections.
    ///
    /// # Errors
    ///
    /// Returns an error for an incompatible schema or an invalid importer identity/version.
    pub fn validate_and_sort(&mut self) -> Result<(), AssetError> {
        if self.format_version != RMETA_FORMAT_VERSION {
            return Err(AssetError::InvalidData(format!(
                "unsupported .rmeta version {}; expected {RMETA_FORMAT_VERSION}",
                self.format_version
            )));
        }
        if self.importer_id.trim().is_empty() || self.importer_version == 0 {
            return Err(AssetError::InvalidData(
                "importer ID and non-zero version are required".to_owned(),
            ));
        }
        self.dependencies.sort_unstable();
        self.dependencies.dedup();
        Ok(())
    }

    /// Hashes the complete import recipe, including dependency artifact hashes.
    pub fn recipe_hash(&self, source_hash: &str, dependency_hashes: &[String]) -> String {
        let mut hashes = dependency_hashes.to_vec();
        hashes.sort();
        let canonical = ron::ser::to_string_pretty(
            &(
                self.importer_id.as_str(),
                self.importer_version,
                &self.settings,
                source_hash,
                hashes,
            ),
            ron::ser::PrettyConfig::new(),
        )
        .unwrap_or_default();
        format!("{:x}", Sha256::digest(canonical.as_bytes()))
    }
}

/// Returns `source.ext.rmeta`, preserving the full source filename.
pub fn sidecar_path(source: &Path) -> PathBuf {
    let mut name = source.file_name().unwrap_or_default().to_os_string();
    name.push(".rmeta");
    source.with_file_name(name)
}

/// Validates a path relative to an asset root before any filesystem mutation.
///
/// # Errors
///
/// Returns an error for empty, absolute, prefixed, rooted, or parent-traversing paths.
pub fn validate_asset_relative_path(path: &Path) -> Result<(), AssetError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(AssetError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

/// Reads and validates a bounded sidecar.
///
/// # Errors
///
/// Returns an error when the file cannot be read, exceeds the size limit, is malformed, or fails
/// schema validation.
pub fn load_meta(path: &Path) -> Result<AssetMeta, AssetError> {
    let metadata = fs::metadata(path).map_err(|error| io_error(path, error))?;
    if metadata.len() > MAX_RMETA_BYTES {
        return Err(AssetError::Limit(format!(
            "sidecar is {} bytes; maximum is {MAX_RMETA_BYTES}",
            metadata.len()
        )));
    }
    let text = fs::read_to_string(path).map_err(|error| io_error(path, error))?;
    let mut meta: AssetMeta =
        ron::from_str(&text).map_err(|error| AssetError::InvalidData(error.to_string()))?;
    meta.validate_and_sort()?;
    Ok(meta)
}

/// Writes a sidecar through a same-directory temporary file and recoverable backup.
///
/// # Errors
///
/// Returns an error when validation, serialization, or the transactional filesystem write fails.
pub fn save_meta(path: &Path, meta: &AssetMeta) -> Result<(), AssetError> {
    let mut validated = meta.clone();
    validated.validate_and_sort()?;
    let text = ron::ser::to_string_pretty(&validated, ron::ser::PrettyConfig::new())
        .map_err(|error| AssetError::InvalidData(error.to_string()))?;
    atomic_write(path, format!("{text}\n").as_bytes())
}

/// SHA-256 hexadecimal content hash.
pub fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AssetError> {
    let parent = path
        .parent()
        .ok_or_else(|| AssetError::UnsafePath(path.to_path_buf()))?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    let token = Uuid::new_v4();
    let temporary = parent.join(format!(".asset-{token}.tmp"));
    let backup = parent.join(format!(".asset-{token}.bak"));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| io_error(&temporary, error))?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| io_error(&temporary, error))?;
        file.sync_all()
            .map_err(|error| io_error(&temporary, error))?;
        drop(file);
        if path.exists() {
            fs::rename(path, &backup).map_err(|error| io_error(path, error))?;
            if let Err(error) = fs::rename(&temporary, path) {
                let _ = fs::rename(&backup, path);
                return Err(io_error(path, error));
            }
            fs::remove_file(&backup).map_err(|error| io_error(&backup, error))?;
        } else {
            fs::rename(&temporary, path).map_err(|error| io_error(path, error))?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_is_adjacent_and_preserves_full_filename() {
        assert_eq!(
            sidecar_path(Path::new("assets/player.png")),
            PathBuf::from("assets/player.png.rmeta")
        );
    }

    #[test]
    fn recipe_hash_is_independent_of_dependency_order() {
        let meta = AssetMeta::new("rustic.texture", 1);
        assert_eq!(
            meta.recipe_hash("source", &["b".to_owned(), "a".to_owned()]),
            meta.recipe_hash("source", &["a".to_owned(), "b".to_owned()])
        );
    }

    #[test]
    fn sidecar_round_trip_preserves_stable_id() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("image.png.rmeta");
        let meta = AssetMeta::new("rustic.texture", 2);
        save_meta(&path, &meta).unwrap();
        assert_eq!(load_meta(&path).unwrap(), meta);
    }

    #[test]
    fn traversal_is_rejected() {
        for path in ["../outside.png", "/absolute.png", ""] {
            assert!(validate_asset_relative_path(Path::new(path)).is_err());
        }
    }
}
