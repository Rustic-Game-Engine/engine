//! Disposable SQLite index, checksummed DDC, moves, duplicate quarantine, and reports.

use crate::artifacts::DerivedArtifact;
use crate::meta::{AssetMeta, atomic_write, load_meta, save_meta, sidecar_path};
use crate::{AssetError, content_hash, io_error, validate_asset_relative_path};
use engine_core::AssetId;
use rusqlite::{Connection, OptionalExtension as _, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr as _;
use uuid::Uuid;

/// Current disposable SQLite schema version.
pub const ASSET_INDEX_SCHEMA_VERSION: u32 = 1;
/// Current checksummed artifact envelope version.
pub const DDC_FORMAT_VERSION: u32 = 1;

/// Cached record rebuilt from authoritative source plus `.rmeta` data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetRecord {
    /// Stable source identity.
    pub id: AssetId,
    /// Asset-root-relative source path.
    pub source_path: PathBuf,
    /// Last source content hash.
    pub source_hash: Option<String>,
    /// Last complete recipe hash.
    pub recipe_hash: Option<String>,
    /// Importer identifier.
    pub importer_id: String,
    /// Importer version.
    pub importer_version: u32,
}

/// Rebuildable local asset index. `.rmeta` remains authoritative.
pub struct AssetIndex {
    path: PathBuf,
    connection: Connection,
}

impl AssetIndex {
    /// Opens/migrates the index. Corrupt databases are quarantined and replaced.
    ///
    /// # Errors
    ///
    /// Returns an error when the database directory cannot be prepared or SQLite cannot validate,
    /// migrate, quarantine, or recreate the index.
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AssetError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
        }
        match Self::open_once(&path) {
            Ok(index) => Ok(index),
            Err(first_error) if path.exists() => {
                let quarantine = path.with_extension(format!("corrupt-{}", Uuid::new_v4()));
                fs::rename(&path, &quarantine).map_err(|error| io_error(&path, error))?;
                Self::open_once(&path).map_err(|second_error| {
                    AssetError::Index(format!(
                        "index was quarantined at {}; original error: {first_error}; rebuild error: {second_error}",
                        quarantine.display()
                    ))
                })
            }
            Err(error) => Err(error),
        }
    }

    fn open_once(path: &Path) -> Result<Self, AssetError> {
        let connection =
            Connection::open(path).map_err(|error| AssetError::Index(error.to_string()))?;
        let check: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(|error| AssetError::Index(error.to_string()))?;
        if check != "ok" {
            return Err(AssetError::Index(format!("SQLite quick_check: {check}")));
        }
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|error| AssetError::Index(error.to_string()))?;
        match version {
            0 => {
                connection
                    .execute_batch(
                        "BEGIN IMMEDIATE;
                         CREATE TABLE IF NOT EXISTS assets (
                           asset_id TEXT PRIMARY KEY NOT NULL,
                           source_path TEXT NOT NULL UNIQUE,
                           source_hash TEXT,
                           recipe_hash TEXT,
                           importer_id TEXT NOT NULL,
                           importer_version INTEGER NOT NULL
                         );
                         CREATE TABLE IF NOT EXISTS dependencies (
                           asset_id TEXT NOT NULL,
                           dependency_id TEXT NOT NULL,
                           PRIMARY KEY(asset_id, dependency_id)
                         );
                         CREATE INDEX IF NOT EXISTS dependencies_reverse
                           ON dependencies(dependency_id);
                         PRAGMA user_version = 1;
                         COMMIT;",
                    )
                    .map_err(|error| AssetError::Index(error.to_string()))?;
            }
            ASSET_INDEX_SCHEMA_VERSION => {}
            future => {
                return Err(AssetError::Index(format!(
                    "future index schema {future}; current is {ASSET_INDEX_SCHEMA_VERSION}"
                )));
            }
        }
        Ok(Self {
            path: path.to_path_buf(),
            connection,
        })
    }

    /// Index file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Inserts/replaces one cached record and its dependency edges transactionally.
    ///
    /// # Errors
    ///
    /// Returns an error when the SQLite transaction cannot be executed or committed.
    pub fn upsert(
        &mut self,
        record: &AssetRecord,
        dependencies: &[AssetId],
    ) -> Result<(), AssetError> {
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| AssetError::Index(error.to_string()))?;
        transaction
            .execute(
                "INSERT INTO assets(asset_id, source_path, source_hash, recipe_hash, importer_id, importer_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(asset_id) DO UPDATE SET
                   source_path=excluded.source_path,
                   source_hash=excluded.source_hash,
                   recipe_hash=excluded.recipe_hash,
                   importer_id=excluded.importer_id,
                   importer_version=excluded.importer_version",
                params![
                    record.id.to_string(),
                    record.source_path.to_string_lossy(),
                    record.source_hash,
                    record.recipe_hash,
                    record.importer_id,
                    record.importer_version,
                ],
            )
            .map_err(|error| AssetError::Index(error.to_string()))?;
        transaction
            .execute(
                "DELETE FROM dependencies WHERE asset_id=?1",
                params![record.id.to_string()],
            )
            .map_err(|error| AssetError::Index(error.to_string()))?;
        let mut sorted = dependencies.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        for dependency in sorted {
            transaction
                .execute(
                    "INSERT INTO dependencies(asset_id, dependency_id) VALUES (?1, ?2)",
                    params![record.id.to_string(), dependency.to_string()],
                )
                .map_err(|error| AssetError::Index(error.to_string()))?;
        }
        transaction
            .commit()
            .map_err(|error| AssetError::Index(error.to_string()))
    }

    /// Finds one cached asset by stable ID.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot execute the lookup or stored data is invalid.
    pub fn get(&self, id: AssetId) -> Result<Option<AssetRecord>, AssetError> {
        self.connection
            .query_row(
                "SELECT asset_id, source_path, source_hash, recipe_hash, importer_id, importer_version
                 FROM assets WHERE asset_id=?1",
                params![id.to_string()],
                record_from_row,
            )
            .optional()
            .map_err(|error| AssetError::Index(error.to_string()))
    }

    /// Returns records in stable path order.
    ///
    /// # Errors
    ///
    /// Returns an error when SQLite cannot read the index or a stored record is invalid.
    pub fn all(&self) -> Result<Vec<AssetRecord>, AssetError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT asset_id, source_path, source_hash, recipe_hash, importer_id, importer_version
                 FROM assets ORDER BY source_path, asset_id",
            )
            .map_err(|error| AssetError::Index(error.to_string()))?;
        statement
            .query_map([], record_from_row)
            .map_err(|error| AssetError::Index(error.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AssetError::Index(error.to_string()))
    }

    /// Reverse dependency closure, excluding the changed asset itself.
    ///
    /// # Errors
    ///
    /// Returns an error when dependency rows cannot be queried or contain an invalid asset ID.
    pub fn dependents_of(&self, changed: AssetId) -> Result<Vec<AssetId>, AssetError> {
        let mut discovered = BTreeSet::new();
        let mut queue = VecDeque::from([changed]);
        while let Some(dependency) = queue.pop_front() {
            let mut statement = self
                .connection
                .prepare(
                    "SELECT asset_id FROM dependencies WHERE dependency_id=?1 ORDER BY asset_id",
                )
                .map_err(|error| AssetError::Index(error.to_string()))?;
            let values = statement
                .query_map(params![dependency.to_string()], |row| {
                    row.get::<_, String>(0)
                })
                .map_err(|error| AssetError::Index(error.to_string()))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| AssetError::Index(error.to_string()))?;
            for value in values {
                let id = AssetId::from_str(&value)
                    .map_err(|error| AssetError::Index(error.to_string()))?;
                if id != changed && discovered.insert(id) {
                    queue.push_back(id);
                }
            }
        }
        Ok(discovered.into_iter().collect())
    }

    /// Deletes and rebuilds the disposable index from authoritative sidecars.
    ///
    /// # Errors
    ///
    /// Returns an error when the asset tree cannot be scanned or the rebuild transaction fails.
    pub fn rebuild_from_sidecars(&mut self, asset_root: &Path) -> Result<ScanReport, AssetError> {
        let scan = scan_sidecars(asset_root)?;
        let transaction = self
            .connection
            .transaction()
            .map_err(|error| AssetError::Index(error.to_string()))?;
        transaction
            .execute("DELETE FROM dependencies", [])
            .and_then(|_| transaction.execute("DELETE FROM assets", []))
            .map_err(|error| AssetError::Index(error.to_string()))?;
        transaction
            .commit()
            .map_err(|error| AssetError::Index(error.to_string()))?;
        for entry in &scan.entries {
            let record = AssetRecord {
                id: entry.meta.asset_id,
                source_path: entry.source_relative.clone(),
                source_hash: entry.meta.source_hash.clone(),
                recipe_hash: None,
                importer_id: entry.meta.importer_id.clone(),
                importer_version: entry.meta.importer_version,
            };
            self.upsert(&record, &entry.meta.dependencies)?;
        }
        Ok(scan)
    }
}

fn record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AssetRecord> {
    let id: String = row.get(0)?;
    let id = AssetId::from_str(&id).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(AssetRecord {
        id,
        source_path: PathBuf::from(row.get::<_, String>(1)?),
        source_hash: row.get(2)?,
        recipe_hash: row.get(3)?,
        importer_id: row.get(4)?,
        importer_version: row.get(5)?,
    })
}

/// Valid sidecar discovered during an authoritative scan.
#[derive(Debug, Clone)]
pub struct ScannedAsset {
    /// Root-relative source path.
    pub source_relative: PathBuf,
    /// Root-relative sidecar path.
    pub sidecar_relative: PathBuf,
    /// Parsed metadata.
    pub meta: AssetMeta,
}

/// Sidecar scan result with duplicates withheld from indexing.
#[derive(Debug, Clone, Default)]
pub struct ScanReport {
    /// Unique, valid assets.
    pub entries: Vec<ScannedAsset>,
    /// Duplicate ID to all conflicting source paths.
    pub duplicates: BTreeMap<AssetId, Vec<PathBuf>>,
    /// Corrupt/invalid sidecars and diagnostics.
    pub corrupt: Vec<(PathBuf, String)>,
}

/// Scans only regular `.rmeta` files beneath the canonical asset root. Symlinked directories are
/// not followed.
///
/// # Errors
///
/// Returns an error when the root cannot be canonicalized or a directory cannot be enumerated.
pub fn scan_sidecars(asset_root: &Path) -> Result<ScanReport, AssetError> {
    let canonical_root =
        fs::canonicalize(asset_root).map_err(|error| io_error(asset_root, error))?;
    let mut candidates = Vec::new();
    let mut queue = VecDeque::from([canonical_root.clone()]);
    while let Some(directory) = queue.pop_front() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|error| io_error(&directory, error))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| io_error(&directory, error))?;
        entries.sort_by_key(fs::DirEntry::path);
        for entry in entries {
            let file_type = entry
                .file_type()
                .map_err(|error| io_error(entry.path(), error))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                queue.push_back(entry.path());
            } else if file_type.is_file()
                && entry.path().extension().is_some_and(|extension| {
                    extension.to_string_lossy().eq_ignore_ascii_case("rmeta")
                })
            {
                candidates.push(entry.path());
            }
        }
    }
    let mut report = ScanReport::default();
    let mut by_id: BTreeMap<AssetId, Vec<ScannedAsset>> = BTreeMap::new();
    for sidecar in candidates {
        let relative = sidecar
            .strip_prefix(&canonical_root)
            .map_err(|_| AssetError::UnsafePath(sidecar.clone()))?
            .to_path_buf();
        let source_name = relative
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".rmeta"))
            .map(str::to_owned);
        let Some(source_name) = source_name else {
            report
                .corrupt
                .push((relative, "invalid sidecar filename".to_owned()));
            continue;
        };
        let source_relative = relative.with_file_name(source_name);
        if !canonical_root.join(&source_relative).is_file() {
            report.corrupt.push((
                relative,
                format!("source {} is missing", source_relative.display()),
            ));
            continue;
        }
        match load_meta(&sidecar) {
            Ok(meta) => by_id.entry(meta.asset_id).or_default().push(ScannedAsset {
                source_relative,
                sidecar_relative: relative,
                meta,
            }),
            Err(error) => report.corrupt.push((relative, error.to_string())),
        }
    }
    for (id, mut entries) in by_id {
        entries.sort_by(|left, right| left.source_relative.cmp(&right.source_relative));
        if entries.len() == 1 {
            report.entries.extend(entries);
        } else {
            report.duplicates.insert(
                id,
                entries
                    .into_iter()
                    .map(|entry| entry.source_relative)
                    .collect(),
            );
        }
    }
    report
        .entries
        .sort_by(|left, right| left.source_relative.cmp(&right.source_relative));
    Ok(report)
}

/// Moves one source and its sidecar. If the sidecar move fails, the source move is rolled back.
///
/// # Errors
///
/// Returns an error for unsafe paths, occupied destinations, or failed directory and rename
/// operations.
pub fn move_source_with_meta(
    asset_root: &Path,
    from_relative: &Path,
    to_relative: &Path,
) -> Result<(), AssetError> {
    validate_asset_relative_path(from_relative)?;
    validate_asset_relative_path(to_relative)?;
    let root = fs::canonicalize(asset_root).map_err(|error| io_error(asset_root, error))?;
    let from = root.join(from_relative);
    let to = root.join(to_relative);
    let from_meta = sidecar_path(&from);
    let to_meta = sidecar_path(&to);
    if to.exists() || to_meta.exists() {
        return Err(AssetError::InvalidData(format!(
            "move destination already exists: {}",
            to.display()
        )));
    }
    let parent = to
        .parent()
        .ok_or_else(|| AssetError::UnsafePath(to.clone()))?;
    fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    fs::rename(&from, &to).map_err(|error| io_error(&from, error))?;
    if let Err(error) = fs::rename(&from_meta, &to_meta) {
        let _ = fs::rename(&to, &from);
        return Err(io_error(&from_meta, error));
    }
    Ok(())
}

/// Explicitly remaps one duplicate source to a fresh stable identity.
///
/// # Errors
///
/// Returns an error when the path is unsafe or the authoritative sidecar cannot be read or saved.
pub fn remap_duplicate(asset_root: &Path, source_relative: &Path) -> Result<AssetId, AssetError> {
    validate_asset_relative_path(source_relative)?;
    let path = asset_root.join(source_relative);
    let sidecar = sidecar_path(&path);
    let mut meta = load_meta(&sidecar)?;
    meta.asset_id = AssetId::new();
    meta.source_hash = None;
    save_meta(&sidecar, &meta)?;
    Ok(meta.asset_id)
}

/// One unresolved stable reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MissingReference {
    /// Missing asset.
    pub missing: AssetId,
    /// Resource containing the reference.
    pub referring_resource: String,
    /// Entity/component/property context.
    pub location: String,
}

/// Produces a stable missing-reference report from known IDs and reference contexts.
pub fn missing_reference_report(
    known: impl IntoIterator<Item = AssetId>,
    references: impl IntoIterator<Item = (AssetId, String, String)>,
) -> Vec<MissingReference> {
    let known = known.into_iter().collect::<BTreeSet<_>>();
    let mut report = references
        .into_iter()
        .filter(|(id, _, _)| !known.contains(id))
        .map(|(missing, referring_resource, location)| MissingReference {
            missing,
            referring_resource,
            location,
        })
        .collect::<Vec<_>>();
    report.sort_by(|left, right| {
        left.missing
            .cmp(&right.missing)
            .then_with(|| left.referring_resource.cmp(&right.referring_resource))
            .then_with(|| left.location.cmp(&right.location))
    });
    report
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DdcEnvelope {
    format_version: u32,
    asset_id: AssetId,
    recipe_hash: String,
    payload_checksum: String,
    artifact: DerivedArtifact,
}

/// Checksummed, rebuildable derived-data cache.
#[derive(Debug, Clone)]
pub struct DerivedDataCache {
    root: PathBuf,
}

impl DerivedDataCache {
    /// Creates a cache rooted in a rebuildable project directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Deterministic cache file for a recipe key.
    pub fn path_for(&self, recipe_hash: &str) -> PathBuf {
        let prefix = recipe_hash.get(..2).unwrap_or("00");
        self.root.join(prefix).join(format!("{recipe_hash}.rddc"))
    }

    /// Atomically writes an artifact envelope.
    ///
    /// # Errors
    ///
    /// Returns an error when the artifact cannot be serialized or installed in the cache.
    pub fn store(
        &self,
        asset_id: AssetId,
        recipe_hash: &str,
        artifact: &DerivedArtifact,
    ) -> Result<PathBuf, AssetError> {
        let payload = ron::ser::to_string_pretty(artifact, ron::ser::PrettyConfig::new())
            .map_err(|error| AssetError::InvalidData(error.to_string()))?;
        let envelope = DdcEnvelope {
            format_version: DDC_FORMAT_VERSION,
            asset_id,
            recipe_hash: recipe_hash.to_owned(),
            payload_checksum: format!("{:x}", Sha256::digest(payload.as_bytes())),
            artifact: artifact.clone(),
        };
        let text = ron::ser::to_string_pretty(&envelope, ron::ser::PrettyConfig::new())
            .map_err(|error| AssetError::InvalidData(error.to_string()))?;
        let path = self.path_for(recipe_hash);
        atomic_write(&path, format!("{text}\n").as_bytes())?;
        Ok(path)
    }

    /// Loads and verifies a cache envelope. Any error means callers rebuild from source.
    ///
    /// # Errors
    ///
    /// Returns an error when the entry is missing, malformed, incompatible, or fails identity or
    /// checksum validation.
    pub fn load(
        &self,
        asset_id: AssetId,
        recipe_hash: &str,
    ) -> Result<DerivedArtifact, AssetError> {
        let path = self.path_for(recipe_hash);
        let bytes = fs::read(&path).map_err(|error| io_error(&path, error))?;
        let envelope: DdcEnvelope = ron::de::from_bytes(&bytes)
            .map_err(|error| AssetError::InvalidData(error.to_string()))?;
        if envelope.format_version != DDC_FORMAT_VERSION
            || envelope.asset_id != asset_id
            || envelope.recipe_hash != recipe_hash
        {
            return Err(AssetError::InvalidData(
                "DDC envelope identity/version mismatch".to_owned(),
            ));
        }
        let payload = ron::ser::to_string_pretty(&envelope.artifact, ron::ser::PrettyConfig::new())
            .map_err(|error| AssetError::InvalidData(error.to_string()))?;
        if content_hash(payload.as_bytes()) != envelope.payload_checksum {
            return Err(AssetError::InvalidData(
                "DDC artifact checksum mismatch".to_owned(),
            ));
        }
        Ok(envelope.artifact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetKind, PlaceholderArtifact};

    fn write_source_with_meta(root: &Path, relative: &str, meta: &AssetMeta) {
        let source = root.join(relative);
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"source").unwrap();
        save_meta(&sidecar_path(&source), meta).unwrap();
    }

    #[test]
    fn disposable_index_rebuilds_and_invalidates_only_dependents() {
        let directory = tempfile::tempdir().unwrap();
        let mut index = AssetIndex::open(directory.path().join("assets.sqlite")).unwrap();
        let a = AssetId::new();
        let b = AssetId::new();
        let c = AssetId::new();
        for (id, path, dependencies) in [(a, "a", vec![]), (b, "b", vec![a]), (c, "c", vec![])] {
            index
                .upsert(
                    &AssetRecord {
                        id,
                        source_path: path.into(),
                        source_hash: None,
                        recipe_hash: None,
                        importer_id: "test".to_owned(),
                        importer_version: 1,
                    },
                    &dependencies,
                )
                .unwrap();
        }
        assert_eq!(index.dependents_of(a).unwrap(), vec![b]);
        assert!(index.dependents_of(c).unwrap().is_empty());
    }

    #[test]
    fn duplicate_ids_are_withheld_until_explicit_remap() {
        let directory = tempfile::tempdir().unwrap();
        let meta = AssetMeta::new("rustic.image", 1);
        write_source_with_meta(directory.path(), "a.png", &meta);
        write_source_with_meta(directory.path(), "b.png", &meta);
        let scan = scan_sidecars(directory.path()).unwrap();
        assert!(scan.entries.is_empty());
        assert_eq!(scan.duplicates[&meta.asset_id].len(), 2);
        let new_id = remap_duplicate(directory.path(), Path::new("b.png")).unwrap();
        assert_ne!(new_id, meta.asset_id);
        let scan = scan_sidecars(directory.path()).unwrap();
        assert_eq!(scan.entries.len(), 2);
    }

    #[test]
    fn moving_source_and_meta_preserves_identity() {
        let directory = tempfile::tempdir().unwrap();
        let meta = AssetMeta::new("rustic.image", 1);
        write_source_with_meta(directory.path(), "old/a.png", &meta);
        move_source_with_meta(
            directory.path(),
            Path::new("old/a.png"),
            Path::new("new/a.png"),
        )
        .unwrap();
        assert_eq!(
            load_meta(&sidecar_path(&directory.path().join("new/a.png")))
                .unwrap()
                .asset_id,
            meta.asset_id
        );
        assert!(!directory.path().join("old/a.png").exists());
    }

    #[test]
    fn ddc_corruption_is_detected() {
        let directory = tempfile::tempdir().unwrap();
        let cache = DerivedDataCache::new(directory.path());
        let id = AssetId::new();
        let artifact = DerivedArtifact::Placeholder(PlaceholderArtifact {
            asset_id: id,
            kind: AssetKind::Texture,
            reason: None,
        });
        let path = cache.store(id, "abcdef", &artifact).unwrap();
        assert_eq!(cache.load(id, "abcdef").unwrap(), artifact);
        fs::write(path, b"corrupt").unwrap();
        assert!(cache.load(id, "abcdef").is_err());
    }

    #[test]
    fn missing_reference_report_is_localized_and_sorted() {
        let known = AssetId::new();
        let missing = AssetId::new();
        let report = missing_reference_report(
            [known],
            [
                (known, "scene".to_owned(), "entity/a".to_owned()),
                (missing, "scene".to_owned(), "entity/b.mesh".to_owned()),
            ],
        );
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].missing, missing);
        assert_eq!(report[0].location, "entity/b.mesh");
    }
}
