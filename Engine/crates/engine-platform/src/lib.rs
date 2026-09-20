//! Engine-owned native platform services.

mod atomic_file;
mod config;
mod crash_marker;
mod paths;
mod process;
mod vfs;

pub use atomic_file::{AtomicFileService, AtomicWriteError, AtomicWriteOptions, RecoveryAction};
pub use config::{ConfigDocument, ConfigError, ConfigLoad, ConfigSchema, ConfigStore};
pub use crash_marker::{CrashMarkerError, StartupMarker, StartupRecord};
pub use paths::{NativePlatformPaths, PlatformError, PlatformPaths};
pub use process::{
    NativeProcessLauncher, ProcessError, ProcessExit, ProcessLauncher, ProcessRequest,
    SpawnedProcess, SupervisedProcess,
};
pub use vfs::{MountTable, VfsError, VfsMount, VfsPath};
