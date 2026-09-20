use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
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
        match create_lock(&path) {
            Ok(lock) => Ok(ProjectAccess::Writable(lock)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let record = fs::read_to_string(&path)
                    .ok()
                    .and_then(|value| ron::from_str::<LockRecord>(&value).ok());
                if record.as_ref().is_some_and(|record| {
                    record.format_version == LOCK_FORMAT_VERSION
                        && process_is_alive(record.process_id)
                }) {
                    return Ok(ProjectAccess::ReadOnly {
                        owner_process_id: record.map(|record| record.process_id),
                    });
                }
                let quarantine =
                    path.with_file_name(format!("editor.lock.stale-{}", Uuid::new_v4()));
                fs::rename(&path, &quarantine).map_err(|source| ProjectLockError::Io {
                    path: path.clone(),
                    source,
                })?;
                create_lock(&path)
                    .map(ProjectAccess::Writable)
                    .map_err(|source| ProjectLockError::Io { path, source })
            }
            Err(source) => Err(ProjectLockError::Io { path, source }),
        }
    }
}

fn create_lock(path: &Path) -> Result<ProjectLock, std::io::Error> {
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
    Ok(ProjectLock {
        path: path.to_path_buf(),
        token,
    })
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
    }
}

fn process_is_alive(process_id: u32) -> bool {
    if process_id == std::process::id() {
        return true;
    }
    #[cfg(windows)]
    {
        let filter = format!("PID eq {process_id}");
        Command::new("tasklist")
            .args(["/FI", &filter, "/FO", "CSV", "/NH"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .is_some_and(|output| {
                let text = String::from_utf8_lossy(&output.stdout);
                !text.contains("No tasks") && text.contains(&process_id.to_string())
            })
    }
    #[cfg(not(windows))]
    {
        PathBuf::from("/proc").join(process_id.to_string()).is_dir()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

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
