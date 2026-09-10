//! Data-backed services shared by the native editor UI and headless authoring tests.

mod console;
mod document;
mod lock;
mod viewport;
mod workspace;

pub use console::*;
pub use document::*;
pub use lock::*;
pub use viewport::*;
pub use workspace::*;
