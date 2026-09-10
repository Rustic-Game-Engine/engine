//! Engine-owned boundaries for isolated local play.
//!
//! This crate intentionally contains no editor, renderer-backend, or native-window
//! types. The same protocol and state machines are usable by the native editor,
//! runtime, tests, and future command-line tooling.

pub mod changes;
pub mod frame_ring;
pub mod local_ipc;
pub mod protocol;
pub mod runtime_server;
mod script_runtime;
pub mod simulation;
pub mod snapshot;
pub mod supervisor;

pub use changes::{
    ApplyRuntimeChangesTransaction, MapChangeStore, RuntimeChange, RuntimeChangeError,
    RuntimeChangeSet, RuntimeChangeStore, RuntimeChangeTarget, RuntimeValue,
};
pub use frame_ring::{BgraFrame, FrameRing, FrameRingError, FrameRingLimits, FrameRingStats};
pub use local_ipc::{
    AuthenticatedConnection, LocalEndpoint, LocalIpcListener, connect_authenticated,
};
pub use protocol::{
    AuthenticationToken, ConsoleEvent, ConsoleRecord, IpcConnection, PROTOCOL_VERSION,
    ProcessDescriptor, ProcessRole, ProtocolError, ProtocolMessage, ProtocolVersion,
};
pub use runtime_server::{
    RuntimeServerConfig, RuntimeServerError, record_runtime_crash, run_runtime_server,
};
pub use simulation::{
    ControlAck, ControlRequest, PlayMode, RuntimeSimulation, RuntimeState, SimulationError,
};
pub use snapshot::{
    PlaySnapshot, PlaySnapshotManifest, SnapshotBuilder, SnapshotError, SnapshotInput,
    SnapshotManifestFile,
};
pub use supervisor::{RuntimeLaunch, SupervisedRuntime, SupervisorError, SupervisorExit};
