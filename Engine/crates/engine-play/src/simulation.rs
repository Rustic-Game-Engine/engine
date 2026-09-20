//! Deterministic fixed-step runtime control.

use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

/// The three isolated local play modes exposed by the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayMode {
    /// Runtime renders to a frame transport consumed by the editor viewport.
    Play,
    /// Runtime owns a development preview window.
    NewWindow,
    /// Runtime uses an export-like staged mount set.
    Standalone,
}

impl PlayMode {
    /// Stable command-line name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::NewWindow => "new-window",
            Self::Standalone => "standalone",
        }
    }
}

impl std::str::FromStr for PlayMode {
    type Err = SimulationError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "play" | "embedded" => Ok(Self::Play),
            "new-window" => Ok(Self::NewWindow),
            "standalone" => Ok(Self::Standalone),
            other => Err(SimulationError::InvalidMode(other.to_owned())),
        }
    }
}

/// Observable runtime scheduling state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    /// Fixed simulation ticks advance with elapsed wall time.
    Running,
    /// Simulation time is frozen. One explicit fixed step is allowed.
    Paused,
    /// Runtime is shutting down and cannot advance.
    Stopped,
}

/// Editor-to-runtime control request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlRequest {
    Pause,
    Resume,
    FrameAdvance,
    QueryState,
    Stop,
}

/// State returned after a control request reaches a simulation barrier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlAck {
    pub request: ControlRequest,
    pub state: RuntimeState,
    pub fixed_tick: u64,
}

/// Invalid runtime control or fixed-step configuration.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SimulationError {
    #[error("fixed simulation delta must be non-zero")]
    ZeroFixedDelta,
    #[error("frame advance requires a paused runtime")]
    FrameAdvanceWhileNotPaused,
    #[error("runtime has already stopped")]
    AlreadyStopped,
    #[error("unknown play mode `{0}`")]
    InvalidMode(String),
}

/// Pure runtime clock/state machine.
///
/// The owner calls [`advance_elapsed`](Self::advance_elapsed) from its scheduling
/// loop. Pause, resume, and step operations are synchronous barriers on this value.
#[derive(Debug, Clone)]
pub struct RuntimeSimulation {
    mode: PlayMode,
    state: RuntimeState,
    fixed_delta: Duration,
    accumulator: Duration,
    fixed_tick: u64,
    maximum_catch_up_ticks: u32,
}

impl RuntimeSimulation {
    /// Creates a running fixed-step simulation.
    ///
    /// # Errors
    ///
    /// Returns [`SimulationError::ZeroFixedDelta`] for a zero duration.
    pub fn new(mode: PlayMode, fixed_delta: Duration) -> Result<Self, SimulationError> {
        if fixed_delta.is_zero() {
            return Err(SimulationError::ZeroFixedDelta);
        }
        Ok(Self {
            mode,
            state: RuntimeState::Running,
            fixed_delta,
            accumulator: Duration::ZERO,
            fixed_tick: 0,
            maximum_catch_up_ticks: 8,
        })
    }

    /// Changes the per-update catch-up bound.
    #[must_use]
    pub fn with_maximum_catch_up_ticks(mut self, maximum: u32) -> Self {
        self.maximum_catch_up_ticks = maximum.max(1);
        self
    }

    pub const fn mode(&self) -> PlayMode {
        self.mode
    }

    pub const fn state(&self) -> RuntimeState {
        self.state
    }

    pub const fn fixed_tick(&self) -> u64 {
        self.fixed_tick
    }

    pub const fn fixed_delta(&self) -> Duration {
        self.fixed_delta
    }

    /// Advances zero or more fixed ticks and returns the number executed.
    /// Paused/stopped simulations ignore elapsed time.
    pub fn advance_elapsed(&mut self, elapsed: Duration) -> u32 {
        if self.state != RuntimeState::Running {
            return 0;
        }
        self.accumulator = self.accumulator.saturating_add(elapsed);
        let mut executed = 0;
        while self.accumulator >= self.fixed_delta && executed < self.maximum_catch_up_ticks {
            self.accumulator = self.accumulator.saturating_sub(self.fixed_delta);
            self.fixed_tick = self.fixed_tick.saturating_add(1);
            executed += 1;
        }
        if executed == self.maximum_catch_up_ticks && self.accumulator >= self.fixed_delta {
            // A bounded scheduler discards an excessive backlog rather than spiralling.
            self.accumulator = Duration::ZERO;
        }
        executed
    }

    /// Applies one control request at a simulation barrier.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid state transitions. In particular, frame advance
    /// is accepted only while paused and increments the counter exactly once.
    pub fn control(&mut self, request: ControlRequest) -> Result<ControlAck, SimulationError> {
        match request {
            ControlRequest::Pause => {
                if self.state == RuntimeState::Stopped {
                    return Err(SimulationError::AlreadyStopped);
                }
                self.state = RuntimeState::Paused;
                self.accumulator = Duration::ZERO;
            }
            ControlRequest::Resume => {
                if self.state == RuntimeState::Stopped {
                    return Err(SimulationError::AlreadyStopped);
                }
                self.state = RuntimeState::Running;
                self.accumulator = Duration::ZERO;
            }
            ControlRequest::FrameAdvance => {
                if self.state != RuntimeState::Paused {
                    return Err(SimulationError::FrameAdvanceWhileNotPaused);
                }
                self.fixed_tick = self.fixed_tick.saturating_add(1);
            }
            ControlRequest::QueryState => {}
            ControlRequest::Stop => {
                self.state = RuntimeState::Stopped;
                self.accumulator = Duration::ZERO;
            }
        }
        Ok(ControlAck {
            request,
            state: self.state,
            fixed_tick: self.fixed_tick,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simulation() -> RuntimeSimulation {
        RuntimeSimulation::new(PlayMode::Play, Duration::from_millis(10)).unwrap()
    }

    #[test]
    fn pause_freezes_elapsed_time_and_one_step_is_exact() {
        let mut simulation = simulation();
        assert_eq!(simulation.advance_elapsed(Duration::from_millis(35)), 3);
        assert_eq!(simulation.fixed_tick(), 3);

        simulation.control(ControlRequest::Pause).unwrap();
        assert_eq!(simulation.advance_elapsed(Duration::from_secs(10)), 0);
        assert_eq!(simulation.fixed_tick(), 3);

        let stepped = simulation.control(ControlRequest::FrameAdvance).unwrap();
        assert_eq!(stepped.fixed_tick, 4);
        assert_eq!(stepped.state, RuntimeState::Paused);
        assert_eq!(simulation.advance_elapsed(Duration::from_secs(10)), 0);
        assert_eq!(simulation.fixed_tick(), 4);
    }

    #[test]
    fn resume_advances_again_and_stop_is_terminal() {
        let mut simulation = simulation();
        simulation.control(ControlRequest::Pause).unwrap();
        simulation.control(ControlRequest::Resume).unwrap();
        assert_eq!(simulation.advance_elapsed(Duration::from_millis(10)), 1);
        simulation.control(ControlRequest::Stop).unwrap();
        assert_eq!(simulation.advance_elapsed(Duration::from_secs(1)), 0);
        assert_eq!(
            simulation.control(ControlRequest::Resume),
            Err(SimulationError::AlreadyStopped)
        );
    }

    #[test]
    fn frame_advance_rejects_running_state_without_mutation() {
        let mut simulation = simulation();
        assert_eq!(
            simulation.control(ControlRequest::FrameAdvance),
            Err(SimulationError::FrameAdvanceWhileNotPaused)
        );
        assert_eq!(simulation.fixed_tick(), 0);
        assert_eq!(simulation.state(), RuntimeState::Running);
    }

    #[test]
    fn catch_up_is_bounded() {
        let mut simulation = simulation().with_maximum_catch_up_ticks(2);
        assert_eq!(simulation.advance_elapsed(Duration::from_secs(1)), 2);
        assert_eq!(simulation.advance_elapsed(Duration::ZERO), 0);
    }
}
