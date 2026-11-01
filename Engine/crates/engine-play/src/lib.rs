//! Engine-owned boundaries for isolated local play.
//!
//! This crate intentionally contains no editor, renderer-backend, or native-window
//! types. The same protocol and state machines are usable by the native editor,
//! runtime, tests, and future command-line tooling.

pub mod changes;
mod entity_services;
pub mod frame_ring;
#[cfg(feature = "native")]
pub mod local_ipc;
mod physics;
pub mod pointer_input;
pub mod protocol;
#[cfg(feature = "native")]
pub mod runtime_server;
mod script_runtime;
pub mod simulation;
pub mod snapshot;
#[cfg(feature = "native")]
pub mod supervisor;
pub mod viewport;
pub use viewport::{MAX_GAME_DIMENSION, game_view_extent};

pub use changes::{
    ApplyRuntimeChangesTransaction, MapChangeStore, RuntimeChange, RuntimeChangeError,
    RuntimeChangeSet, RuntimeChangeStore, RuntimeChangeTarget, RuntimeValue,
};
pub use frame_ring::{BgraFrame, FrameRing, FrameRingError, FrameRingLimits, FrameRingStats};
#[cfg(feature = "native")]
pub use local_ipc::{
    AuthenticatedConnection, LocalEndpoint, LocalIpcListener, connect_authenticated,
};
pub use protocol::{
    AuthenticationToken, ConsoleEvent, ConsoleRecord, IpcConnection, LiveEntityProperties,
    PROTOCOL_VERSION, ProcessDescriptor, ProcessRole, ProtocolError, ProtocolMessage,
    ProtocolVersion,
};
#[cfg(feature = "native")]
pub use runtime_server::{
    RuntimeServerConfig, RuntimeServerError, record_runtime_crash, run_runtime_server,
    run_runtime_server_with_renderer, run_runtime_server_with_viewport_renderer,
};
pub use simulation::{
    ControlAck, ControlRequest, PlayMode, RuntimeSimulation, RuntimeState, SimulationError,
};
pub use snapshot::{
    PlaySnapshot, PlaySnapshotManifest, SnapshotBuilder, SnapshotError, SnapshotInput,
    SnapshotManifestFile,
};
#[cfg(feature = "native")]
pub use supervisor::{RuntimeLaunch, SupervisedRuntime, SupervisorError, SupervisorExit};

mod audio_output;
mod gameplay_world;

mod audio_capture;
mod audio_input;

pub mod game_session;
