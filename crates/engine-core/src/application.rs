//! Shared application identity and lifecycle types.

use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fmt;

use crate::{EngineError, ErrorCategory};

/// Shared command-line switch used by every native process smoke test.
pub const HEADLESS_SMOKE_ARGUMENT: &str = "--headless-smoke";
/// Test-only process switch proving that smoke failures reach the operating system.
pub const HEADLESS_SMOKE_FAILURE_ARGUMENT: &str = "--headless-smoke-fail";

/// The semantic version of the engine data/API contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EngineVersion {
    /// Breaking format/API generation.
    pub major: u16,
    /// Backwards-compatible feature generation.
    pub minor: u16,
    /// Patch generation.
    pub patch: u16,
}

impl EngineVersion {
    /// Version of the engine that produced this binary.
    pub const CURRENT: Self = Self {
        major: 0,
        minor: 1,
        patch: 0,
    };

    /// Returns whether data authored by `other` may be opened without a migration.
    pub const fn is_format_compatible_with(self, other: Self) -> bool {
        self.major == other.major && self.minor >= other.minor
    }
}

impl fmt::Display for EngineVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Distinguishes isolated executable responsibilities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationRole {
    /// Project discovery and creation only.
    ProjectManager,
    /// Authoring UI; supervises play sessions.
    Editor,
    /// Embedded or out-of-process development game runtime.
    Runtime,
    /// Exported game runtime.
    Standalone,
    /// Sandboxed helper for a language adapter.
    ScriptHost,
}

/// Stable metadata included in diagnostics from every process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationIdentity {
    /// Human-readable process name.
    pub name: String,
    /// Responsibility of this process.
    pub role: ApplicationRole,
    /// Engine build version.
    pub engine_version: EngineVersion,
}

impl ApplicationIdentity {
    /// Creates an identity for the current engine version.
    pub fn new(name: impl Into<String>, role: ApplicationRole) -> Self {
        Self {
            name: name.into(),
            role,
            engine_version: EngineVersion::CURRENT,
        }
    }

    /// Validates the minimum process-entry identity needed by diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when required identity fields are invalid.
    pub fn validate(&self) -> Result<(), EngineError> {
        if self.name.trim().is_empty() {
            return Err(EngineError::new(
                ErrorCategory::InvalidInput,
                "application identity name cannot be empty",
            ));
        }
        Ok(())
    }
}

/// Returns whether the shared process smoke-test switch is present.
pub fn headless_smoke_requested(arguments: impl IntoIterator<Item = impl AsRef<OsStr>>) -> bool {
    arguments
        .into_iter()
        .any(|argument| argument.as_ref() == HEADLESS_SMOKE_ARGUMENT)
}

/// Runs the common non-UI process-entry validation used in CI.
///
/// # Errors
///
/// Returns [`EngineError`] when the process identity is invalid.
pub fn run_headless_smoke(identity: &ApplicationIdentity) -> Result<(), EngineError> {
    identity.validate()?;
    println!(
        "headless smoke: {} ({:?}) engine {}",
        identity.name, identity.role, identity.engine_version
    );
    Ok(())
}

/// Handles the shared smoke switches and returns whether the process should exit.
///
/// # Errors
///
/// Returns a deterministic injected error for [`HEADLESS_SMOKE_FAILURE_ARGUMENT`],
/// or propagates normal identity validation failures.
pub fn run_headless_smoke_from_arguments(
    identity: &ApplicationIdentity,
    arguments: impl IntoIterator<Item = impl AsRef<OsStr>>,
) -> Result<bool, EngineError> {
    let arguments: Vec<_> = arguments
        .into_iter()
        .map(|argument| argument.as_ref().to_os_string())
        .collect();
    if arguments
        .iter()
        .any(|argument| argument == HEADLESS_SMOKE_FAILURE_ARGUMENT)
    {
        return Err(EngineError::new(
            ErrorCategory::Internal,
            "injected headless smoke failure",
        ));
    }
    if headless_smoke_requested(&arguments) {
        run_headless_smoke(identity)?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatibility_requires_matching_major_and_new_enough_minor() {
        let current = EngineVersion {
            major: 1,
            minor: 4,
            patch: 2,
        };

        assert!(current.is_format_compatible_with(EngineVersion {
            major: 1,
            minor: 3,
            patch: 99,
        }));
        assert!(!current.is_format_compatible_with(EngineVersion {
            major: 1,
            minor: 5,
            patch: 0,
        }));
        assert!(!current.is_format_compatible_with(EngineVersion {
            major: 2,
            minor: 0,
            patch: 0,
        }));
    }

    #[test]
    fn process_smoke_contract_is_explicit_and_validates_identity() {
        assert!(headless_smoke_requested(["--headless-smoke"]));
        assert!(!headless_smoke_requested(["--headless"]));
        let identity = ApplicationIdentity::new("", ApplicationRole::Editor);
        assert_eq!(
            identity.validate().unwrap_err().category(),
            ErrorCategory::InvalidInput
        );
    }

    #[test]
    fn injected_smoke_failure_is_an_error() {
        let identity = ApplicationIdentity::new("editor", ApplicationRole::Editor);
        assert!(
            run_headless_smoke_from_arguments(&identity, [HEADLESS_SMOKE_FAILURE_ARGUMENT])
                .is_err()
        );
    }
}
