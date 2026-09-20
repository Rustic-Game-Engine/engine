//! Common error taxonomy used at application and service boundaries.

use thiserror::Error;

/// Stable, broad category suitable for diagnostics and process protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCategory {
    /// User-provided data or arguments are invalid.
    InvalidInput,
    /// A requested resource does not exist.
    NotFound,
    /// The operating system refused or failed an operation.
    Platform,
    /// Persisted data could not be read or written safely.
    Persistence,
    /// A versioned contract is incompatible.
    Compatibility,
    /// An internal invariant was violated.
    Internal,
}

/// Foundation error carrying a category and safe human-readable context.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{category:?}: {message}")]
pub struct EngineError {
    category: ErrorCategory,
    message: String,
}

impl EngineError {
    /// Creates a categorized engine error.
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }

    /// Broad machine-readable category.
    pub const fn category(&self) -> ErrorCategory {
        self.category
    }

    /// Human-readable context safe to surface in native interfaces.
    pub fn message(&self) -> &str {
        &self.message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_retains_category_and_context() {
        let error = EngineError::new(ErrorCategory::Persistence, "catalog install failed");
        assert_eq!(error.category(), ErrorCategory::Persistence);
        assert_eq!(error.message(), "catalog install failed");
        assert!(error.to_string().contains("catalog install failed"));
    }
}
