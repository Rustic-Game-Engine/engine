//! Cooperative cancellation shared by background services.

use parking_lot::{Condvar, Mutex};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Default)]
struct CancellationState {
    cancelled: AtomicBool,
    wait_lock: Mutex<()>,
    changed: Condvar,
}

/// Cloneable observer passed to cancellable work.
#[derive(Debug, Clone, Default)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}

impl CancellationToken {
    /// Returns whether cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    /// Waits until cancellation or the timeout, returning whether cancellation won.
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        if self.is_cancelled() {
            return true;
        }
        let mut guard = self.state.wait_lock.lock();
        self.state.changed.wait_for(&mut guard, timeout);
        self.is_cancelled()
    }
}

/// Owner capable of requesting cancellation for its tokens.
#[derive(Debug, Clone, Default)]
pub struct CancellationSource {
    token: CancellationToken,
}

impl CancellationSource {
    /// Creates an uncancelled source.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a token sharing this source's state.
    pub fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    /// Requests cancellation. Repeated calls are harmless.
    pub fn cancel(&self) {
        if !self.token.state.cancelled.swap(true, Ordering::AcqRel) {
            self.token.state.changed.notify_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloned_tokens_observe_idempotent_cancellation() {
        let source = CancellationSource::new();
        let first = source.token();
        let second = first.clone();
        source.cancel();
        source.cancel();
        assert!(first.is_cancelled());
        assert!(second.wait_timeout(Duration::from_millis(1)));
    }
}
