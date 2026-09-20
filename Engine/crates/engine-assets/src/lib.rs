//! Versioned source-asset metadata, importers, dependency index, cache, and async loading.

mod artifacts;
mod database;
mod importers;
mod meta;
mod pipeline;
mod watcher;
mod worker;

pub use artifacts::*;
pub use database::*;
pub use importers::*;
pub use meta::*;
pub use pipeline::*;
pub use watcher::*;
pub use worker::*;

use std::path::PathBuf;
use thiserror::Error;

/// Shared failures produced by the asset pipeline.
#[derive(Debug, Error)]
pub enum AssetError {
    /// A source or sidecar path is outside the configured asset root.
    #[error("unsafe asset path: {0}")]
    UnsafePath(PathBuf),
    /// An input exceeded the configured resource limits.
    #[error("asset limit exceeded: {0}")]
    Limit(String),
    /// The extension has no registered importer.
    #[error("unsupported asset format: {0}")]
    UnsupportedFormat(String),
    /// Source bytes are malformed or inconsistent.
    #[error("asset decode failed: {0}")]
    Decode(String),
    /// Versioned persisted data is malformed or incompatible.
    #[error("asset metadata/cache is invalid: {0}")]
    InvalidData(String),
    /// Filesystem operation failed with path context.
    #[error("asset I/O failed at {path}: {source}")]
    Io {
        /// Affected path.
        path: PathBuf,
        /// Native error.
        #[source]
        source: std::io::Error,
    },
    /// Disposable SQLite index operation failed.
    #[error("asset index failed: {0}")]
    Index(String),
    /// Isolated worker process failed, crashed, timed out, or returned invalid data.
    #[error("asset worker failed: {0}")]
    Worker(String),
}

pub(crate) fn io_error(path: impl Into<PathBuf>, source: std::io::Error) -> AssetError {
    AssetError::Io {
        path: path.into(),
        source,
    }
}
