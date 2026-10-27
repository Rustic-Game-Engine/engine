use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

const LOCK_FORMAT_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
struct LockRecord {
    format_version: u32,
    process_id: u32,
    token: Uuid,
}

#[derive(Debug, Error)]
pub enum ProjectLockError {
    #[error("project lock I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("project lock could not be serialized: {0}")]
    Serialize(String),
}

/// Result of opening the editor lock. A live owner yields a read-only session instead of
/// allowing two processes to write the same authoring data.
#[derive(Debug)]
pub enum ProjectAccess {
    Writable(ProjectLock),
    ReadOnly { owner_process_id: Option<u32> },
}

#[derive(Debug)]
pub struct ProjectLock {
    path: PathBuf,
    token: Uuid,
    // Keep a stable inode: deleting this file would let concurrent openers lock
    // different files at the same path. The OS releases it even after a crash.
    writer: File,
}

impl ProjectLock {
    /// Acquires a writer lock, reports a live owner as read-only, or quarantines stale metadata.
    ///
    /// # Errors
    ///
    /// Returns an error when lock metadata cannot be inspected, quarantined, or installed.
    pub fn acquire(project_root: &Path) -> Result<ProjectAccess, ProjectLockError> {
        let path = project_root.join("temp").join("editor.lock");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| ProjectLockError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let writer_path = path.with_file_name("editor.writer.lock");
        let writer = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&writer_path)
            .map_err(|source| ProjectLockError::Io {
                path: writer_path.clone(),
                source,
            })?;
        match writer.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                let owner_process_id = fs::read_to_string(&path)
                    .ok()
                    .and_then(|value| ron::from_str::<LockRecord>(&value).ok())
                    .map(|record| record.process_id);
                return Ok(ProjectAccess::ReadOnly { owner_process_id });
            }
            Err(TryLockError::Error(source)) => {
                return Err(ProjectLockError::Io {
                    path: writer_path,
                    source,
                });
            }
        }
        match create_lock(&path) {
            Ok(token) => Ok(ProjectAccess::Writable(Self {
                path,
                token,
                writer,
            })),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let quarantine =
                    path.with_file_name(format!("editor.lock.stale-{}", Uuid::new_v4()));
                fs::rename(&path, &quarantine).map_err(|source| ProjectLockError::Io {
                    path: path.clone(),
                    source,
                })?;
                create_lock(&path)
                    .map(|token| {
                        ProjectAccess::Writable(Self {
                            path: path.clone(),
                            token,
                            writer,
                        })
                    })
                    .map_err(|source| ProjectLockError::Io { path, source })
            }
            Err(source) => Err(ProjectLockError::Io { path, source }),
        }
    }
}

fn create_lock(path: &Path) -> Result<Uuid, std::io::Error> {
    let token = Uuid::new_v4();
    let record = LockRecord {
        format_version: LOCK_FORMAT_VERSION,
        process_id: std::process::id(),
        token,
    };
    let text =
        ron::ser::to_string(&record).map_err(|error| std::io::Error::other(error.to_string()))?;
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    Ok(token)
}

impl Drop for ProjectLock {
    fn drop(&mut self) {
        let owned = fs::read_to_string(&self.path)
            .ok()
            .and_then(|value| ron::from_str::<LockRecord>(&value).ok())
            .is_some_and(|record| record.token == self.token);
        if owned {
            let _ = fs::remove_file(&self.path);
        }
        // A concurrent Unix fork can briefly inherit this open file description
        // before exec closes it. Unlock explicitly so those inherited handles do
        // not keep a closed editor's project read-only.
        let _ = self.writer.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn lock_is_released_when_owner_exits_without_dropping() {
        const CHILD_ROOT: &str = "RUSTIC_LOCK_TEST_CHILD_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            let access = ProjectLock::acquire(Path::new(&root)).unwrap();
            assert!(matches!(access, ProjectAccess::Writable(_)));
            // Simulate abrupt termination: ProjectLock::drop does not run.
            std::process::exit(0);
        }
        let temporary = tempdir().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "lock::tests::lock_is_released_when_owner_exits_without_dropping",
                "--nocapture",
            ])
            .env(CHILD_ROOT, temporary.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(temporary.path().join("temp/editor.lock").is_file());
        assert!(matches!(
            ProjectLock::acquire(temporary.path()).unwrap(),
            ProjectAccess::Writable(_)
        ));
    }

    #[test]
    fn stale_metadata_with_reused_live_pid_does_not_make_project_read_only() {
        let temporary = tempdir().unwrap();
        fs::create_dir_all(temporary.path().join("temp")).unwrap();
        let path = temporary.path().join("temp/editor.lock");
        let record = LockRecord {
            format_version: LOCK_FORMAT_VERSION,
            process_id: std::process::id(),
            token: Uuid::new_v4(),
        };
        fs::write(&path, ron::ser::to_string(&record).unwrap()).unwrap();
        assert!(matches!(
            ProjectLock::acquire(temporary.path()).unwrap(),
            ProjectAccess::Writable(_)
        ));
    }

    #[test]
    fn live_lock_is_read_only_and_drop_releases_it() {
        let temporary = tempdir().unwrap();
        let first = ProjectLock::acquire(temporary.path()).unwrap();
        assert!(matches!(first, ProjectAccess::Writable(_)));
        assert!(matches!(
            ProjectLock::acquire(temporary.path()).unwrap(),
            ProjectAccess::ReadOnly { .. }
        ));
        drop(first);
        assert!(matches!(
            ProjectLock::acquire(temporary.path()).unwrap(),
            ProjectAccess::Writable(_)
        ));
    }

    #[test]
    fn dropping_owner_unlocks_even_when_a_duplicate_handle_remains_open() {
        let temporary = tempdir().unwrap();
        let ProjectAccess::Writable(owner) = ProjectLock::acquire(temporary.path()).unwrap() else {
            panic!("the first editor must acquire the writer lock");
        };
        // A concurrent Unix process spawn can temporarily inherit this same open
        // file description between fork and exec. A clone reproduces that lifetime.
        let duplicate = owner.writer.try_clone().unwrap();
        assert!(matches!(
            ProjectLock::acquire(temporary.path()).unwrap(),
            ProjectAccess::ReadOnly { .. }
        ));
        drop(owner);
        assert!(matches!(
            ProjectLock::acquire(temporary.path()).unwrap(),
            ProjectAccess::Writable(_)
        ));
        drop(duplicate);
    }

    #[test]
    fn malformed_lock_is_quarantined_and_recovered() {
        let temporary = tempdir().unwrap();
        fs::create_dir_all(temporary.path().join("temp")).unwrap();
        fs::write(temporary.path().join("temp/editor.lock"), "damaged").unwrap();
        assert!(matches!(
            ProjectLock::acquire(temporary.path()).unwrap(),
            ProjectAccess::Writable(_)
        ));
        assert!(
            fs::read_dir(temporary.path().join("temp"))
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .contains("stale"))
        );
    }
}
