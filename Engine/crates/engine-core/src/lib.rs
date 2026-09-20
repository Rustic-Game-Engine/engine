//! Foundational, platform-independent services for Rustic Game Engine.

pub mod application;
pub mod cancellation;
pub mod clock;
pub mod error;
pub mod ids;
pub mod jobs;
pub mod logging;

pub use application::{
    ApplicationIdentity, ApplicationRole, EngineVersion, HEADLESS_SMOKE_ARGUMENT,
    HEADLESS_SMOKE_FAILURE_ARGUMENT, headless_smoke_requested, run_headless_smoke,
    run_headless_smoke_from_arguments,
};
pub use cancellation::{CancellationSource, CancellationToken};
pub use clock::{Clock, ClockSnapshot, ManualClock, SystemClock};
pub use error::{EngineError, ErrorCategory};
pub use ids::{AssetId, EntityId, ProjectId, SceneId, ScriptId, WorldId};
pub use jobs::{JobError, JobHandle, JobPriority, JobScheduler, JobSchedulerStats, SubmitError};
