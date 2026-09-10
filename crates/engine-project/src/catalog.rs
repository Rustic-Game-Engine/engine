use crate::{Project, ProjectId, ProjectTemplate};
use engine_platform::AtomicFileService;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

pub const CURRENT_CATALOG_VERSION: u32 = 2;
const MAX_CATALOG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_THUMBNAIL_BYTES: usize = 8 * 1024 * 1024;

/// Cached project data used by the UI without filesystem access per frame.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: ProjectId,
    pub path: PathBuf,
    pub last_opened_unix_seconds: u64,
    #[serde(default = "unavailable_name")]
    pub name: String,
    #[serde(default)]
    pub template: ProjectTemplate,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engine_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<ThumbnailRef>,
    #[serde(default)]
    pub available: bool,
}

impl ProjectRecord {
    pub fn from_project(project: &Project) -> Self {
        Self {
            id: project.id(),
            path: project.root().to_path_buf(),
            last_opened_unix_seconds: unix_seconds(),
            name: project.metadata().name.clone(),
            template: project.descriptor().template,
            engine_version: project.metadata().engine_version.clone(),
            thumbnail: None,
            available: true,
        }
    }
}

fn unavailable_name() -> String {
    "Unavailable".to_owned()
}

/// Versioned launcher catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCatalog {
    pub format_version: u32,
    #[serde(default)]
    pub projects: Vec<ProjectRecord>,
}

impl Default for ProjectCatalog {
    fn default() -> Self {
        Self {
            format_version: CURRENT_CATALOG_VERSION,
            projects: Vec::new(),
        }
    }
}

/// Stable catalog sort modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogSort {
    Recent,
    Name,
    Location,
}

/// Recovery or migration performed while loading.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogLoadStatus {
    Missing,
    Current,
    Migrated { from_version: u32 },
    RecoveredBackup { quarantine: PathBuf },
    RecoveredEmpty { quarantine: PathBuf },
}

/// Loaded catalog plus user-visible recovery state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogLoad {
    pub catalog: ProjectCatalog,
    pub status: CatalogLoadStatus,
}

/// Catalog persistence or compatibility failure.
#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("catalog I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("catalog at {path} is corrupt: {message}")]
    Corrupt { path: PathBuf, message: String },
    #[error("catalog version {found} is newer than supported version {supported}")]
    FutureVersion { found: u32, supported: u32 },
    #[error("catalog serialization failed: {0}")]
    Serialize(String),
    #[error("catalog exceeds the {MAX_CATALOG_BYTES}-byte safety limit: {0}")]
    TooLarge(PathBuf),
    #[error("project operation failed: {0}")]
    Project(#[from] crate::ProjectError),
    #[error("transactional catalog operation failed: {0}")]
    Atomic(#[from] engine_platform::AtomicWriteError),
}

impl ProjectCatalog {
    /// Loads with migration, interrupted-write recovery, and corruption quarantine.
    ///
    /// # Errors
    ///
    /// Returns an error when catalog recovery, parsing, migration, quarantine, or persistence
    /// fails, or when a future schema is encountered.
    pub fn load(path: &Path) -> Result<CatalogLoad, CatalogError> {
        let files = AtomicFileService::new();
        files.recover(path)?;
        if !path.exists() {
            return Ok(CatalogLoad {
                catalog: Self::default(),
                status: CatalogLoadStatus::Missing,
            });
        }
        match parse_catalog(path) {
            Ok(load) => Ok(load),
            Err(error @ CatalogError::FutureVersion { .. }) => Err(error),
            Err(primary_error) => {
                let backup = AtomicFileService::backup_path(path);
                let recovered = backup
                    .is_file()
                    .then(|| parse_catalog(&backup).ok())
                    .flatten();
                let quarantine = quarantine_path(path);
                fs::rename(path, &quarantine).map_err(|source| CatalogError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
                if let Some(mut recovered) = recovered {
                    if !files.restore_backup(path)? {
                        return Err(primary_error);
                    }
                    recovered.status = CatalogLoadStatus::RecoveredBackup { quarantine };
                    return Ok(recovered);
                }
                let catalog = Self::default();
                catalog.save(path)?;
                Ok(CatalogLoad {
                    catalog,
                    status: CatalogLoadStatus::RecoveredEmpty { quarantine },
                })
            }
        }
    }

    /// Saves current schema bytes transactionally with a retained prior backup.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or transactional persistence fails.
    pub fn save(&self, path: &Path) -> Result<(), CatalogError> {
        let mut normalized = self.clone();
        normalized.format_version = CURRENT_CATALOG_VERSION;
        normalized.normalize();
        let mut text = ron::ser::to_string_pretty(
            &normalized,
            ron::ser::PrettyConfig::new()
                .depth_limit(12)
                .separate_tuple_members(true),
        )
        .map_err(|error| CatalogError::Serialize(error.to_string()))?;
        text.push('\n');
        AtomicFileService::new().write(path, text.as_bytes())?;
        Ok(())
    }

    /// Imports and returns a fully validated project.
    ///
    /// # Errors
    ///
    /// Returns an error when the descriptor cannot be opened, recovered, or validated.
    pub fn import(&mut self, path: &Path) -> Result<Project, CatalogError> {
        let project = Project::open(path)?;
        self.add_or_touch(&project);
        Ok(project)
    }

    pub fn add_or_touch(&mut self, project: &Project) {
        let now = unix_seconds();
        if let Some(existing) = self
            .projects
            .iter_mut()
            .find(|entry| entry.id == project.id() || paths_equivalent(&entry.path, project.root()))
        {
            let thumbnail = existing.thumbnail.clone();
            *existing = ProjectRecord::from_project(project);
            existing.last_opened_unix_seconds = now;
            existing.thumbnail = thumbnail;
        } else {
            self.projects.push(ProjectRecord::from_project(project));
        }
        self.normalize();
    }

    pub fn remove(&mut self, id: ProjectId) -> bool {
        let prior = self.projects.len();
        self.projects.retain(|entry| entry.id != id);
        prior != self.projects.len()
    }

    pub fn contains_path(&self, path: &Path) -> bool {
        self.projects
            .iter()
            .any(|record| paths_equivalent(&record.path, path))
    }

    /// Search/sort operates only on cached record data and performs no I/O.
    pub fn query(&self, search: &str, sort: CatalogSort) -> Vec<&ProjectRecord> {
        let query = search.trim().to_lowercase();
        let mut results: Vec<_> = self
            .projects
            .iter()
            .filter(|record| {
                query.is_empty()
                    || record.name.to_lowercase().contains(&query)
                    || record
                        .path
                        .to_string_lossy()
                        .to_lowercase()
                        .contains(&query)
            })
            .collect();
        results.sort_by(|left, right| match sort {
            CatalogSort::Recent => right
                .last_opened_unix_seconds
                .cmp(&left.last_opened_unix_seconds)
                .then_with(|| left.id.cmp(&right.id)),
            CatalogSort::Name => left
                .name
                .to_lowercase()
                .cmp(&right.name.to_lowercase())
                .then_with(|| left.id.cmp(&right.id)),
            CatalogSort::Location => left
                .path
                .cmp(&right.path)
                .then_with(|| left.id.cmp(&right.id)),
        });
        results
    }

    /// Refreshes cached metadata; unavailable records remain visible and removable.
    pub fn refresh_record(&mut self, id: ProjectId) {
        let Some(index) = self.projects.iter().position(|record| record.id == id) else {
            return;
        };
        let path = self.projects[index].path.clone();
        match Project::open(&path) {
            Ok(project) => {
                let last_opened = self.projects[index].last_opened_unix_seconds;
                let thumbnail = self.projects[index].thumbnail.clone();
                self.projects[index] = ProjectRecord::from_project(&project);
                self.projects[index].last_opened_unix_seconds = last_opened;
                self.projects[index].thumbnail = thumbnail;
            }
            Err(_) => self.projects[index].available = false,
        }
    }

    fn normalize(&mut self) {
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        self.projects.retain(|record| {
            ids.insert(record.id)
                && paths.insert(normalized_path_key(&record.path))
                && !record.name.trim().is_empty()
        });
    }
}

#[derive(Debug, Deserialize)]
struct CatalogHeader {
    format_version: u32,
}

#[derive(Debug, Deserialize)]
struct LegacyCatalog {
    projects: Vec<LegacyProjectRecord>,
}

#[derive(Debug, Deserialize)]
struct LegacyProjectRecord {
    id: ProjectId,
    path: PathBuf,
    last_opened_unix_seconds: u64,
}

fn parse_catalog(path: &Path) -> Result<CatalogLoad, CatalogError> {
    let metadata = fs::metadata(path).map_err(|source| CatalogError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_CATALOG_BYTES {
        return Err(CatalogError::TooLarge(path.to_path_buf()));
    }
    let text = fs::read_to_string(path).map_err(|source| CatalogError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if let Ok(header) = ron::from_str::<CatalogHeader>(&text) {
        if header.format_version > CURRENT_CATALOG_VERSION {
            return Err(CatalogError::FutureVersion {
                found: header.format_version,
                supported: CURRENT_CATALOG_VERSION,
            });
        }
        if header.format_version != CURRENT_CATALOG_VERSION {
            return Err(CatalogError::Corrupt {
                path: path.to_path_buf(),
                message: format!("no migration from version {}", header.format_version),
            });
        }
        let mut catalog: ProjectCatalog =
            ron::from_str(&text).map_err(|error| CatalogError::Corrupt {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?;
        catalog.normalize();
        return Ok(CatalogLoad {
            catalog,
            status: CatalogLoadStatus::Current,
        });
    }
    let legacy: LegacyCatalog = ron::from_str(&text).map_err(|error| CatalogError::Corrupt {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let catalog = ProjectCatalog {
        format_version: CURRENT_CATALOG_VERSION,
        projects: legacy
            .projects
            .into_iter()
            .map(|record| ProjectRecord {
                id: record.id,
                path: record.path,
                last_opened_unix_seconds: record.last_opened_unix_seconds,
                name: unavailable_name(),
                template: ProjectTemplate::default(),
                engine_version: None,
                thumbnail: None,
                available: false,
            })
            .collect(),
    };
    Ok(CatalogLoad {
        catalog,
        status: CatalogLoadStatus::Migrated { from_version: 1 },
    })
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    normalized_path_key(left) == normalized_path_key(right)
}

fn normalized_path_key(path: &Path) -> String {
    let absolute = fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    let key = absolute.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        key.to_lowercase()
    } else {
        key
    }
}

fn quarantine_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map_or_else(|| "projects.ron".into(), std::ffi::OsStr::to_os_string);
    name.push(format!(".corrupt-{}", Uuid::new_v4()));
    path.with_file_name(name)
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Versioned reference to one cached thumbnail blob.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThumbnailRef {
    pub format_version: u32,
    pub cache_file: String,
    pub byte_length: u64,
}

/// Untrusted thumbnail-cache error.
#[derive(Debug, Error)]
pub enum ThumbnailError {
    #[error("thumbnail exceeds the {MAX_THUMBNAIL_BYTES}-byte safety limit")]
    TooLarge,
    #[error("invalid thumbnail cache filename: {0}")]
    InvalidReference(String),
    #[error("thumbnail cache I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("thumbnail cache entry length does not match its metadata")]
    LengthMismatch,
    #[error("transactional thumbnail write failed: {0}")]
    Atomic(#[from] engine_platform::AtomicWriteError),
}

/// Disposable, bounded per-user thumbnail blob cache.
#[derive(Debug, Clone)]
pub struct ThumbnailCache {
    root: PathBuf,
    files: AtomicFileService,
}

impl ThumbnailCache {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            files: AtomicFileService::new(),
        }
    }

    /// Stores bounded thumbnail bytes under a revisioned cache key.
    ///
    /// # Errors
    ///
    /// Returns an error when the payload exceeds policy or the atomic write fails.
    pub fn put(
        &self,
        project_id: ProjectId,
        revision: u64,
        bytes: &[u8],
    ) -> Result<ThumbnailRef, ThumbnailError> {
        if bytes.len() > MAX_THUMBNAIL_BYTES {
            return Err(ThumbnailError::TooLarge);
        }
        let cache_file = format!("{project_id}-{revision:016x}.thumb");
        let path = self.root.join(&cache_file);
        self.files.write(&path, bytes)?;
        Ok(ThumbnailRef {
            format_version: 1,
            cache_file,
            byte_length: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        })
    }

    /// Reads and validates a cached thumbnail.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe reference, I/O failure, or length mismatch.
    pub fn read(&self, reference: &ThumbnailRef) -> Result<Vec<u8>, ThumbnailError> {
        validate_cache_file(&reference.cache_file)?;
        let path = self.root.join(&reference.cache_file);
        let metadata = fs::metadata(&path).map_err(|source| ThumbnailError::Io {
            path: path.clone(),
            source,
        })?;
        if metadata.len() != reference.byte_length
            || metadata.len() > u64::try_from(MAX_THUMBNAIL_BYTES).unwrap_or(u64::MAX)
        {
            return Err(ThumbnailError::LengthMismatch);
        }
        fs::read(&path).map_err(|source| ThumbnailError::Io { path, source })
    }

    /// Removes a cached thumbnail if it exists.
    ///
    /// # Errors
    ///
    /// Returns an error for an unsafe reference or failed file removal.
    pub fn remove(&self, reference: &ThumbnailRef) -> Result<(), ThumbnailError> {
        validate_cache_file(&reference.cache_file)?;
        let path = self.root.join(&reference.cache_file);
        if path.exists() {
            fs::remove_file(&path).map_err(|source| ThumbnailError::Io { path, source })?;
        }
        Ok(())
    }
}

fn validate_cache_file(value: &str) -> Result<(), ThumbnailError> {
    let path = Path::new(value);
    if value.is_empty()
        || path.components().count() != 1
        || path
            .extension()
            .is_none_or(|extension| extension != "thumb")
    {
        return Err(ThumbnailError::InvalidReference(value.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn record(name: &str, seconds: u64) -> ProjectRecord {
        ProjectRecord {
            id: ProjectId::new(),
            path: PathBuf::from(format!("projects/{name}")),
            last_opened_unix_seconds: seconds,
            name: name.to_owned(),
            template: ProjectTemplate::Blank,
            engine_version: None,
            thumbnail: None,
            available: true,
        }
    }

    #[test]
    fn catalog_query_is_deterministic_and_uses_cached_fields() {
        let catalog = ProjectCatalog {
            format_version: CURRENT_CATALOG_VERSION,
            projects: vec![record("Zulu", 1), record("Alpha", 2)],
        };
        assert_eq!(catalog.query("alp", CatalogSort::Name)[0].name, "Alpha");
        assert_eq!(catalog.query("", CatalogSort::Recent)[0].name, "Alpha");
    }

    #[test]
    fn versionless_catalog_migrates() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("projects.ron");
        let id = ProjectId::new();
        fs::write(
            &path,
            format!("(projects:[(id:\"{id}\",path:\"old\",last_opened_unix_seconds:1)])"),
        )
        .unwrap();
        let loaded = ProjectCatalog::load(&path).unwrap();
        assert_eq!(
            loaded.status,
            CatalogLoadStatus::Migrated { from_version: 1 }
        );
        assert_eq!(loaded.catalog.projects[0].id, id);
    }

    #[test]
    fn corrupt_catalog_restores_valid_backup_and_quarantines_bytes() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("projects.ron");
        let mut catalog = ProjectCatalog::default();
        catalog.projects.push(record("First", 1));
        catalog.save(&path).unwrap();
        catalog.projects.push(record("Second", 2));
        catalog.save(&path).unwrap();
        fs::write(&path, "broken").unwrap();
        let loaded = ProjectCatalog::load(&path).unwrap();
        let CatalogLoadStatus::RecoveredBackup { quarantine } = loaded.status else {
            panic!("expected backup recovery");
        };
        assert!(quarantine.is_file());
        assert_eq!(loaded.catalog.projects.len(), 1);
    }

    #[test]
    fn future_catalog_is_never_downgraded_from_backup() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("projects.ron");
        fs::write(&path, "(format_version:99,projects:[])").unwrap();
        assert!(matches!(
            ProjectCatalog::load(&path),
            Err(CatalogError::FutureVersion { found: 99, .. })
        ));
    }

    #[test]
    fn thumbnail_cache_rejects_traversal_and_length_corruption() {
        let directory = tempdir().unwrap();
        let cache = ThumbnailCache::new(directory.path());
        let reference = cache.put(ProjectId::new(), 1, b"rgba").unwrap();
        assert_eq!(cache.read(&reference).unwrap(), b"rgba");
        let malicious = ThumbnailRef {
            format_version: 1,
            cache_file: "../outside.thumb".to_owned(),
            byte_length: 0,
        };
        assert!(matches!(
            cache.read(&malicious),
            Err(ThumbnailError::InvalidReference(_))
        ));
    }
}
