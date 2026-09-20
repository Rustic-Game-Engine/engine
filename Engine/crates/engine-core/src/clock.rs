//! Injectable wall and monotonic clocks.

use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// One coherent observation of wall and monotonic time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSnapshot {
    /// Duration since this clock's monotonic origin.
    pub monotonic: Duration,
    /// UTC nanoseconds since the Unix epoch.
    pub unix_nanos: i128,
}

/// Engine-owned time source used by scheduling and persistence code.
pub trait Clock: Send + Sync {
    /// Obtains a coherent time observation.
    fn now(&self) -> ClockSnapshot;
}

/// Native clock backed by [`Instant`] and [`SystemTime`].
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// Creates a clock whose monotonic origin is now.
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> ClockSnapshot {
        let unix_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| {
                i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX)
            });
        ClockSnapshot {
            monotonic: self.origin.elapsed(),
            unix_nanos,
        }
    }
}

/// Deterministic clock for tests and simulations.
#[derive(Debug, Clone)]
pub struct ManualClock {
    state: Arc<Mutex<ClockSnapshot>>,
}

impl ManualClock {
    /// Creates a manual clock at the supplied wall time.
    pub fn new(unix_nanos: i128) -> Self {
        Self {
            state: Arc::new(Mutex::new(ClockSnapshot {
                monotonic: Duration::ZERO,
                unix_nanos,
            })),
        }
    }

    /// Advances both wall and monotonic time by the same duration.
    pub fn advance(&self, duration: Duration) {
        let mut state = self.state.lock();
        state.monotonic = state.monotonic.saturating_add(duration);
        state.unix_nanos = state
            .unix_nanos
            .saturating_add(i128::try_from(duration.as_nanos()).unwrap_or(i128::MAX));
    }

    /// Sets wall time without moving monotonic time.
    pub fn set_unix_nanos(&self, unix_nanos: i128) {
        self.state.lock().unix_nanos = unix_nanos;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> ClockSnapshot {
        *self.state.lock()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_clock_advances_deterministically() {
        let clock = ManualClock::new(1_000);
        clock.advance(Duration::from_millis(2));
        assert_eq!(
            clock.now(),
            ClockSnapshot {
                monotonic: Duration::from_millis(2),
                unix_nanos: 2_001_000,
            }
        );
    }

    #[test]
    fn wall_clock_adjustment_does_not_rewind_monotonic_time() {
        let clock = ManualClock::new(50);
        clock.advance(Duration::from_secs(1));
        clock.set_unix_nanos(-10);
        assert_eq!(clock.now().monotonic, Duration::from_secs(1));
        assert_eq!(clock.now().unix_nanos, -10);
    }
}
