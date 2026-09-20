use crate::{AtomicFileService, AtomicWriteError};
use engine_core::{Clock, SystemClock};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

const STARTUP_MARKER_VERSION: u32 = 1;

/// Versioned startup record retained only while a process is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartupRecord {
    pub format_version: u32,
    pub process_role: String,
    pub process_id: u32,
    pub started_unix_nanos: i128,
}

/// Startup-marker persistence failure.
#[derive(Debug, Error)]
pub enum CrashMarkerError {
    #[error(transparent)]
    Atomic(#[from] AtomicWriteError),
    #[error("could not serialize startup marker: {0}")]
    Serialize(String),
    #[error("could not clear startup marker {path}: {source}")]
    Clear {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Active startup marker. Call [`StartupMarker::clear`] only after clean shutdown.
#[derive(Debug)]
pub struct StartupMarker {
    path: PathBuf,
    previous_unclean_exit: bool,
    cleared: bool,
}

impl StartupMarker {
    /// Records process startup and reports whether a previous marker was present.
    ///
    /// # Errors
    ///
    /// Returns an error when an existing marker cannot be read or the new marker cannot be
    /// installed transactionally.
    pub fn begin(path: &Path, process_role: impl Into<String>) -> Result<Self, CrashMarkerError> {
        Self::begin_with_clock(path, process_role, &SystemClock::new())
    }

    /// Clock-injected marker creation for deterministic tests.
    ///
    /// # Errors
    ///
    /// Returns an error when marker recovery, serialization, or persistence fails.
    pub fn begin_with_clock(
        path: &Path,
        process_role: impl Into<String>,
        clock: &dyn Clock,
    ) -> Result<Self, CrashMarkerError> {
        let files = AtomicFileService::new();
        files.recover(path)?;
        let previous_unclean_exit = path.is_file();
        let record = StartupRecord {
            format_version: STARTUP_MARKER_VERSION,
            process_role: process_role.into(),
            process_id: std::process::id(),
            started_unix_nanos: clock.now().unix_nanos,
        };
        let mut serialized = ron::to_string(&record)
            .map_err(|error| CrashMarkerError::Serialize(error.to_string()))?;
        serialized.push('\n');
        files.write(path, serialized.as_bytes())?;
        Ok(Self {
            path: path.to_path_buf(),
            previous_unclean_exit,
            cleared: false,
        })
    }

    /// Whether marker creation replaced evidence from an unclean prior session.
    pub const fn previous_unclean_exit(&self) -> bool {
        self.previous_unclean_exit
    }

    /// Removes active and staging markers after orderly shutdown.
    ///
    /// # Errors
    ///
    /// Returns an error when a marker file cannot be removed.
    pub fn clear(&mut self) -> Result<(), CrashMarkerError> {
        if self.cleared {
            return Ok(());
        }
        for path in [
            self.path.clone(),
            AtomicFileService::backup_path(&self.path),
            AtomicFileService::temporary_path(&self.path),
        ] {
            if path.exists() {
                fs::remove_file(&path).map_err(|source| CrashMarkerError::Clear {
                    path: path.clone(),
                    source,
                })?;
            }
        }
        self.cleared = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_core::ManualClock;
    use tempfile::tempdir;

    #[test]
    fn stale_marker_is_detected_and_clean_shutdown_clears_it() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("launcher.startup");
        fs::write(&path, "prior crash").unwrap();
        let mut marker =
            StartupMarker::begin_with_clock(&path, "launcher", &ManualClock::new(42)).unwrap();
        assert!(marker.previous_unclean_exit());
        let record: StartupRecord = ron::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(record.started_unix_nanos, 42);
        marker.clear().unwrap();
        assert!(!path.exists());
    }
}
