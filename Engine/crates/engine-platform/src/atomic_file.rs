use parking_lot::Mutex;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

/// Installation policy for a transactional write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtomicWriteOptions {
    /// Retain the prior complete file beside the target as `<name>.bak`.
    pub keep_backup: bool,
}

impl Default for AtomicWriteOptions {
    fn default() -> Self {
        Self { keep_backup: true }
    }
}

/// Startup repair performed for an interrupted write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryAction {
    None,
    InstalledTemporary,
    RestoredBackup,
    RemovedTemporary,
}

/// Error from a transactional filesystem operation.
#[derive(Debug, Error)]
pub enum AtomicWriteError {
    #[error("atomic file operation failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[cfg(test)]
    #[error("injected atomic-write interruption")]
    Injected,
}

fn io_error(path: impl Into<PathBuf>, source: io::Error) -> AtomicWriteError {
    AtomicWriteError::Io {
        path: path.into(),
        source,
    }
}

/// Reusable same-directory temporary/write/sync/install service.
#[derive(Debug, Clone, Default)]
pub struct AtomicFileService {
    operation_lock: Arc<Mutex<()>>,
}

impl AtomicFileService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn backup_path(target: &Path) -> PathBuf {
        sibling_with_suffix(target, ".bak")
    }

    pub fn temporary_path(target: &Path) -> PathBuf {
        sibling_with_suffix(target, ".tmp")
    }

    /// Atomically replaces `target`, retaining the prior complete file as a backup.
    ///
    /// # Errors
    ///
    /// Returns an error when the parent directory cannot be prepared, the temporary file cannot
    /// be written and synchronized, or the completed file cannot be installed.
    pub fn write(&self, target: &Path, contents: &[u8]) -> Result<(), AtomicWriteError> {
        self.write_with_options(target, contents, AtomicWriteOptions::default())
    }

    /// Atomically replaces `target` according to `options`.
    ///
    /// # Errors
    ///
    /// Returns an error when any filesystem or durability operation fails.
    pub fn write_with_options(
        &self,
        target: &Path,
        contents: &[u8],
        options: AtomicWriteOptions,
    ) -> Result<(), AtomicWriteError> {
        self.write_inner(target, contents, options, |_| false)
    }

    /// Repairs an interrupted transaction for `target` using its temporary file or backup.
    ///
    /// # Errors
    ///
    /// Returns an error when an incomplete transaction cannot be inspected or repaired.
    pub fn recover(&self, target: &Path) -> Result<RecoveryAction, AtomicWriteError> {
        let _operation = self.operation_lock.lock();
        recover_locked(target)
    }

    /// Restores the retained backup only when `target` is missing.
    ///
    /// # Errors
    ///
    /// Returns an error when the backup cannot be installed or its parent synchronized.
    pub fn restore_backup(&self, target: &Path) -> Result<bool, AtomicWriteError> {
        let _operation = self.operation_lock.lock();
        let backup = Self::backup_path(target);
        if target.exists() || !backup.is_file() {
            return Ok(false);
        }
        fs::rename(&backup, target).map_err(|error| io_error(target, error))?;
        sync_parent(target)?;
        Ok(true)
    }

    fn write_inner(
        &self,
        target: &Path,
        contents: &[u8],
        options: AtomicWriteOptions,
        mut interrupted: impl FnMut(AtomicWriteStage) -> bool,
    ) -> Result<(), AtomicWriteError> {
        let _operation = self.operation_lock.lock();
        let parent = target.parent().ok_or_else(|| {
            io_error(
                target,
                io::Error::new(io::ErrorKind::InvalidInput, "target has no parent"),
            )
        })?;
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
        recover_locked(target)?;
        let temporary = Self::temporary_path(target);
        let backup = Self::backup_path(target);
        if temporary.exists() {
            fs::remove_file(&temporary).map_err(|error| io_error(&temporary, error))?;
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| io_error(&temporary, error))?;
        file.write_all(contents)
            .and_then(|()| file.sync_all())
            .map_err(|error| io_error(&temporary, error))?;
        drop(file);
        if interrupted(AtomicWriteStage::TemporarySynced) {
            return Err(interrupted_error(AtomicWriteStage::TemporarySynced));
        }

        let had_target = target.exists();
        if had_target {
            if backup.exists() {
                fs::remove_file(&backup).map_err(|error| io_error(&backup, error))?;
            }
            fs::rename(target, &backup).map_err(|error| io_error(target, error))?;
            sync_parent(target)?;
            if interrupted(AtomicWriteStage::BackupInstalled) {
                return Err(interrupted_error(AtomicWriteStage::BackupInstalled));
            }
        }
        if let Err(install_error) = fs::rename(&temporary, target) {
            if had_target && backup.exists() {
                let _ = fs::rename(&backup, target);
            }
            let _ = fs::remove_file(&temporary);
            return Err(io_error(target, install_error));
        }
        sync_parent(target)?;
        if interrupted(AtomicWriteStage::TargetInstalled) {
            return Err(interrupted_error(AtomicWriteStage::TargetInstalled));
        }
        if had_target && !options.keep_backup && backup.exists() {
            fs::remove_file(&backup).map_err(|error| io_error(&backup, error))?;
            sync_parent(target)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AtomicWriteStage {
    TemporarySynced,
    BackupInstalled,
    TargetInstalled,
}

#[cfg(test)]
fn interrupted_error(stage: AtomicWriteStage) -> AtomicWriteError {
    let _ = stage;
    AtomicWriteError::Injected
}

#[cfg(not(test))]
fn interrupted_error(_stage: AtomicWriteStage) -> AtomicWriteError {
    unreachable!("production writes never inject interruptions")
}

fn recover_locked(target: &Path) -> Result<RecoveryAction, AtomicWriteError> {
    let temporary = AtomicFileService::temporary_path(target);
    let backup = AtomicFileService::backup_path(target);
    if target.exists() {
        if temporary.exists() {
            fs::remove_file(&temporary).map_err(|error| io_error(&temporary, error))?;
            sync_parent(target)?;
            return Ok(RecoveryAction::RemovedTemporary);
        }
        return Ok(RecoveryAction::None);
    }
    if backup.is_file() {
        fs::rename(&backup, target).map_err(|error| io_error(target, error))?;
        if temporary.exists() {
            fs::remove_file(&temporary).map_err(|error| io_error(&temporary, error))?;
        }
        sync_parent(target)?;
        return Ok(RecoveryAction::RestoredBackup);
    }
    if temporary.is_file() {
        fs::rename(&temporary, target).map_err(|error| io_error(target, error))?;
        sync_parent(target)?;
        return Ok(RecoveryAction::InstalledTemporary);
    }
    Ok(RecoveryAction::None)
}

fn sibling_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path
        .file_name()
        .map_or_else(|| "data".into(), std::ffi::OsStr::to_os_string);
    name.push(suffix);
    path.with_file_name(name)
}

fn sync_parent(target: &Path) -> Result<(), AtomicWriteError> {
    let Some(parent) = target.parent() else {
        return Ok(());
    };
    fs::metadata(parent).map_err(|error| io_error(parent, error))?;
    #[cfg(unix)]
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error(parent, error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn replacement_retains_previous_complete_bytes() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("settings.ron");
        let files = AtomicFileService::new();
        files.write(&target, b"first").unwrap();
        files.write(&target, b"second").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second");
        assert_eq!(
            fs::read(AtomicFileService::backup_path(&target)).unwrap(),
            b"first"
        );
    }

    #[test]
    fn interruption_after_backup_restores_prior_target() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("catalog.ron");
        let files = AtomicFileService::new();
        files.write(&target, b"prior").unwrap();
        let result = files.write_inner(
            &target,
            b"replacement",
            AtomicWriteOptions::default(),
            |stage| stage == AtomicWriteStage::BackupInstalled,
        );
        assert!(matches!(result, Err(AtomicWriteError::Injected)));
        assert_eq!(
            files.recover(&target).unwrap(),
            RecoveryAction::RestoredBackup
        );
        assert_eq!(fs::read(&target).unwrap(), b"prior");
    }

    #[test]
    fn interrupted_first_write_installs_synced_temporary() {
        let directory = tempdir().unwrap();
        let target = directory.path().join("first.ron");
        let files = AtomicFileService::new();
        assert!(
            files
                .write_inner(
                    &target,
                    b"complete",
                    AtomicWriteOptions::default(),
                    |stage| stage == AtomicWriteStage::TemporarySynced,
                )
                .is_err()
        );
        assert_eq!(
            files.recover(&target).unwrap(),
            RecoveryAction::InstalledTemporary
        );
        assert_eq!(fs::read(&target).unwrap(), b"complete");
    }
}
