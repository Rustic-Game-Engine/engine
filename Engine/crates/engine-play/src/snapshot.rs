//! Transactional immutable play snapshots.

use crate::simulation::PlayMode;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

pub const SNAPSHOT_FORMAT_VERSION: u32 = 1;
pub const SNAPSHOT_MANIFEST_FILE: &str = "play-snapshot.ron";

/// Caller-owned bytes staged into an isolated runtime mount.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInput {
    pub relative_path: PathBuf,
    pub bytes: Vec<u8>,
}

impl SnapshotInput {
    pub fn new(relative_path: impl Into<PathBuf>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            relative_path: relative_path.into(),
            bytes: bytes.into(),
        }
    }
}

/// Integrity entry for one immutable snapshot file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotManifestFile {
    pub relative_path: PathBuf,
    pub byte_length: u64,
    pub sha256: String,
}

/// Versioned root manifest read by `rustic-runtime` before loading any world data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaySnapshotManifest {
    pub format_version: u32,
    pub session_id: Uuid,
    pub mode: PlayMode,
    pub created_unix_millis: u64,
    pub source_scene: PathBuf,
    pub source_scene_sha256: String,
    pub snapshot_sha256: String,
    pub files: Vec<SnapshotManifestFile>,
}

/// Validated immutable snapshot.
#[derive(Debug, Clone)]
pub struct PlaySnapshot {
    root: PathBuf,
    manifest: PlaySnapshotManifest,
}

impl PlaySnapshot {
    /// Opens and verifies every byte listed by the snapshot manifest.
    ///
    /// # Errors
    ///
    /// Returns localized format, path, I/O, or integrity failures.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, SnapshotError> {
        let root = root.as_ref().to_path_buf();
        if !root.is_dir() {
            return Err(SnapshotError::MissingRoot(root));
        }
        let manifest_path = root.join(SNAPSHOT_MANIFEST_FILE);
        let manifest_text =
            fs::read_to_string(&manifest_path).map_err(|source| SnapshotError::Io {
                path: manifest_path.clone(),
                source,
            })?;
        let manifest: PlaySnapshotManifest = ron::from_str(&manifest_text)
            .map_err(|error| SnapshotError::InvalidManifest(error.to_string()))?;
        if manifest.format_version != SNAPSHOT_FORMAT_VERSION {
            return Err(SnapshotError::UnsupportedVersion {
                found: manifest.format_version,
                supported: SNAPSHOT_FORMAT_VERSION,
            });
        }
        validate_relative_path(&manifest.source_scene)?;
        let mut seen = HashSet::new();
        let mut verified = Vec::with_capacity(manifest.files.len());
        for entry in &manifest.files {
            validate_relative_path(&entry.relative_path)?;
            if !seen.insert(entry.relative_path.clone()) {
                return Err(SnapshotError::DuplicatePath(entry.relative_path.clone()));
            }
            let path = root.join(&entry.relative_path);
            let metadata = fs::symlink_metadata(&path).map_err(|source| SnapshotError::Io {
                path: path.clone(),
                source,
            })?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(SnapshotError::InvalidFileType(path));
            }
            let bytes = fs::read(&path).map_err(|source| SnapshotError::Io {
                path: path.clone(),
                source,
            })?;
            let byte_length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            let actual_hash = hash_bytes(&bytes);
            if byte_length != entry.byte_length || actual_hash != entry.sha256 {
                return Err(SnapshotError::IntegrityMismatch {
                    path: entry.relative_path.clone(),
                    expected: entry.sha256.clone(),
                    actual: actual_hash,
                });
            }
            verified.push((entry.relative_path.clone(), actual_hash));
        }
        if !seen.contains(&manifest.source_scene) {
            return Err(SnapshotError::SourceSceneMissing(
                manifest.source_scene.clone(),
            ));
        }
        let scene_entry = manifest
            .files
            .iter()
            .find(|entry| entry.relative_path == manifest.source_scene)
            .ok_or_else(|| SnapshotError::SourceSceneMissing(manifest.source_scene.clone()))?;
        if scene_entry.sha256 != manifest.source_scene_sha256 {
            return Err(SnapshotError::IntegrityMismatch {
                path: manifest.source_scene.clone(),
                expected: manifest.source_scene_sha256.clone(),
                actual: scene_entry.sha256.clone(),
            });
        }
        let aggregate = aggregate_hash(&verified);
        if aggregate != manifest.snapshot_sha256 {
            return Err(SnapshotError::SnapshotHashMismatch {
                expected: manifest.snapshot_sha256.clone(),
                actual: aggregate,
            });
        }
        Ok(Self { root, manifest })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub const fn manifest(&self) -> &PlaySnapshotManifest {
        &self.manifest
    }

    pub fn source_scene_hash(&self) -> &str {
        &self.manifest.source_scene_sha256
    }

    /// Reads the verified scene copy, never the authoring source file.
    ///
    /// # Errors
    ///
    /// Returns an I/O error annotated with the snapshot path.
    pub fn scene_bytes(&self) -> Result<Vec<u8>, SnapshotError> {
        let path = self.root.join(&self.manifest.source_scene);
        fs::read(&path).map_err(|source| SnapshotError::Io { path, source })
    }

    /// Deletes this session snapshot after making Windows read-only files removable.
    ///
    /// # Errors
    ///
    /// Returns any recursive cleanup I/O failure.
    pub fn remove(self) -> Result<(), SnapshotError> {
        make_tree_removable(&self.root)?;
        fs::remove_dir_all(&self.root).map_err(|source| SnapshotError::Io {
            path: self.root,
            source,
        })
    }
}

/// Transactional builder rooted beneath a project's `temp/play` mount.
#[derive(Debug, Clone)]
pub struct SnapshotBuilder {
    play_directory: PathBuf,
}

impl SnapshotBuilder {
    pub fn new(play_directory: impl Into<PathBuf>) -> Self {
        Self {
            play_directory: play_directory.into(),
        }
    }

    /// Stages, flushes, atomically installs, marks read-only, and verifies a snapshot.
    ///
    /// # Errors
    ///
    /// Rejects unsafe/duplicate paths before mutation. Any staging failure removes only
    /// the uniquely named incomplete directory.
    pub fn stage(
        &self,
        mode: PlayMode,
        scene: SnapshotInput,
        additional_files: &[SnapshotInput],
    ) -> Result<PlaySnapshot, SnapshotError> {
        let mut inputs = Vec::with_capacity(additional_files.len() + 1);
        inputs.push(scene);
        inputs.extend_from_slice(additional_files);
        let mut seen = HashSet::new();
        for input in &inputs {
            validate_relative_path(&input.relative_path)?;
            if input.relative_path == Path::new(SNAPSHOT_MANIFEST_FILE) {
                return Err(SnapshotError::ReservedPath(input.relative_path.clone()));
            }
            if !seen.insert(input.relative_path.clone()) {
                return Err(SnapshotError::DuplicatePath(input.relative_path.clone()));
            }
        }
        let source_scene = inputs[0].relative_path.clone();
        fs::create_dir_all(&self.play_directory).map_err(|source| SnapshotError::Io {
            path: self.play_directory.clone(),
            source,
        })?;
        let session_id = Uuid::new_v4();
        let staging = self
            .play_directory
            .join(format!(".session-{session_id}.staging"));
        let installed = self.play_directory.join(format!("session-{session_id}"));
        fs::create_dir(&staging).map_err(|source| SnapshotError::Io {
            path: staging.clone(),
            source,
        })?;

        let result = (|| {
            let mut files = Vec::with_capacity(inputs.len());
            for input in &inputs {
                let path = staging.join(&input.relative_path);
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|source| SnapshotError::Io {
                        path: parent.to_path_buf(),
                        source,
                    })?;
                }
                write_new_synced(&path, &input.bytes)?;
                files.push(SnapshotManifestFile {
                    relative_path: input.relative_path.clone(),
                    byte_length: u64::try_from(input.bytes.len()).unwrap_or(u64::MAX),
                    sha256: hash_bytes(&input.bytes),
                });
            }
            files.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
            let source_scene_sha256 = files
                .iter()
                .find(|file| file.relative_path == source_scene)
                .ok_or_else(|| SnapshotError::SourceSceneMissing(source_scene.clone()))?
                .sha256
                .clone();
            let verified_pairs: Vec<_> = files
                .iter()
                .map(|file| (file.relative_path.clone(), file.sha256.clone()))
                .collect();
            let manifest = PlaySnapshotManifest {
                format_version: SNAPSHOT_FORMAT_VERSION,
                session_id,
                mode,
                created_unix_millis: unix_millis(),
                source_scene,
                source_scene_sha256,
                snapshot_sha256: aggregate_hash(&verified_pairs),
                files,
            };
            let manifest_text =
                ron::ser::to_string_pretty(&manifest, ron::ser::PrettyConfig::new().depth_limit(8))
                    .map_err(|error| SnapshotError::Serialize(error.to_string()))?;
            write_new_synced(
                &staging.join(SNAPSHOT_MANIFEST_FILE),
                format!("{manifest_text}\n").as_bytes(),
            )?;
            mark_tree_read_only(&staging)?;
            fs::rename(&staging, &installed).map_err(|source| SnapshotError::Io {
                path: installed.clone(),
                source,
            })?;
            PlaySnapshot::open(&installed)
        })();

        if result.is_err() && staging.exists() {
            let _ = make_tree_removable(&staging);
            let _ = fs::remove_dir_all(&staging);
        }
        result
    }
}

/// Snapshot validation and transactional staging failures.
#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("snapshot root does not exist or is not a directory: {0}")]
    MissingRoot(PathBuf),
    #[error("snapshot I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("snapshot manifest is invalid: {0}")]
    InvalidManifest(String),
    #[error("snapshot manifest could not be serialized: {0}")]
    Serialize(String),
    #[error("snapshot version {found} is unsupported; expected {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },
    #[error("unsafe snapshot-relative path: {0}")]
    UnsafePath(PathBuf),
    #[error("snapshot path is duplicated: {0}")]
    DuplicatePath(PathBuf),
    #[error("snapshot path is reserved for the manifest: {0}")]
    ReservedPath(PathBuf),
    #[error("snapshot entry is not a regular non-symlink file: {0}")]
    InvalidFileType(PathBuf),
    #[error("source scene is not listed in the snapshot: {0}")]
    SourceSceneMissing(PathBuf),
    #[error("snapshot file integrity failed for {path}; expected {expected}, got {actual}")]
    IntegrityMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    #[error("snapshot aggregate hash mismatch; expected {expected}, got {actual}")]
    SnapshotHashMismatch { expected: String, actual: String },
}

fn validate_relative_path(path: &Path) -> Result<(), SnapshotError> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(SnapshotError::UnsafePath(path.to_path_buf()));
    }
    Ok(())
}

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), SnapshotError> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|source| SnapshotError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(bytes).map_err(|source| SnapshotError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    file.sync_all().map_err(|source| SnapshotError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn aggregate_hash(entries: &[(PathBuf, String)]) -> String {
    let mut hasher = Sha256::new();
    for (path, hash) in entries {
        let path = path.to_string_lossy();
        hasher.update(path.len().to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update(hash.as_bytes());
    }
    hash_bytes(&hasher.finalize())
}

fn mark_tree_read_only(root: &Path) -> Result<(), SnapshotError> {
    for entry in fs::read_dir(root).map_err(|source| SnapshotError::Io {
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| SnapshotError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            mark_tree_read_only(&path)?;
        } else {
            let mut permissions = entry
                .metadata()
                .map_err(|source| SnapshotError::Io {
                    path: path.clone(),
                    source,
                })?
                .permissions();
            permissions.set_readonly(true);
            fs::set_permissions(&path, permissions)
                .map_err(|source| SnapshotError::Io { path, source })?;
        }
    }
    Ok(())
}

#[cfg(windows)]
#[allow(
    clippy::permissions_set_readonly_false,
    reason = "Windows requires clearing FILE_ATTRIBUTE_READONLY before session cleanup"
)]
fn make_tree_removable(root: &Path) -> Result<(), SnapshotError> {
    for entry in fs::read_dir(root).map_err(|source| SnapshotError::Io {
        path: root.to_path_buf(),
        source,
    })? {
        let entry = entry.map_err(|source| SnapshotError::Io {
            path: root.to_path_buf(),
            source,
        })?;
        let path = entry.path();
        if path.is_dir() {
            make_tree_removable(&path)?;
        } else {
            let mut permissions = entry
                .metadata()
                .map_err(|source| SnapshotError::Io {
                    path: path.clone(),
                    source,
                })?
                .permissions();
            permissions.set_readonly(false);
            fs::set_permissions(&path, permissions)
                .map_err(|source| SnapshotError::Io { path, source })?;
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn make_tree_removable(root: &Path) -> Result<(), SnapshotError> {
    if root.is_dir() {
        Ok(())
    } else {
        Err(SnapshotError::MissingRoot(root.to_path_buf()))
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage(directory: &Path) -> PlaySnapshot {
        SnapshotBuilder::new(directory)
            .stage(
                PlayMode::Play,
                SnapshotInput::new("scenes/main.rscene", b"scene-v1".to_vec()),
                &[SnapshotInput::new("assets/mesh.bin", b"mesh-v1".to_vec())],
            )
            .unwrap()
    }

    #[test]
    fn staged_snapshot_is_verified_and_source_bytes_are_copied() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = stage(directory.path());
        assert_eq!(snapshot.scene_bytes().unwrap(), b"scene-v1");
        assert_eq!(snapshot.manifest().files.len(), 2);
        assert_eq!(
            PlaySnapshot::open(snapshot.root())
                .unwrap()
                .source_scene_hash()
                .len(),
            64
        );
        snapshot.remove().unwrap();
    }

    #[test]
    fn traversal_and_duplicates_are_rejected_before_staging() {
        let directory = tempfile::tempdir().unwrap();
        let builder = SnapshotBuilder::new(directory.path());
        assert!(matches!(
            builder.stage(
                PlayMode::Play,
                SnapshotInput::new("../outside", Vec::new()),
                &[]
            ),
            Err(SnapshotError::UnsafePath(_))
        ));
        assert!(matches!(
            builder.stage(
                PlayMode::Play,
                SnapshotInput::new("same", Vec::new()),
                &[SnapshotInput::new("same", Vec::new())]
            ),
            Err(SnapshotError::DuplicatePath(_))
        ));
        assert!(fs::read_dir(directory.path()).unwrap().next().is_none());
    }

    #[test]
    fn tampered_file_is_detected() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = stage(directory.path());
        let path = snapshot.root().join("scenes/main.rscene");
        make_test_writable(&path);
        fs::write(&path, b"tampered").unwrap();
        assert!(matches!(
            PlaySnapshot::open(snapshot.root()),
            Err(SnapshotError::IntegrityMismatch { .. })
        ));
        snapshot.remove().unwrap();
    }

    #[test]
    fn future_manifest_version_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let snapshot = stage(directory.path());
        let path = snapshot.root().join(SNAPSHOT_MANIFEST_FILE);
        make_test_writable(&path);
        let text = fs::read_to_string(&path).unwrap();
        fs::write(
            &path,
            text.replacen("format_version: 1", "format_version: 999", 1),
        )
        .unwrap();
        assert!(matches!(
            PlaySnapshot::open(snapshot.root()),
            Err(SnapshotError::UnsupportedVersion { found: 999, .. })
        ));
        snapshot.remove().unwrap();
    }

    #[cfg(unix)]
    fn make_test_writable(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[cfg(windows)]
    #[allow(
        clippy::permissions_set_readonly_false,
        reason = "the Windows corruption fixture must modify a staged read-only file"
    )]
    fn make_test_writable(path: &Path) {
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions).unwrap();
    }
}
