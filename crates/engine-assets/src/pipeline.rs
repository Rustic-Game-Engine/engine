//! Bounded background import handles with last-good DDC and typed placeholders.

use crate::{
    AssetKind, AssetMeta, DerivedArtifact, DerivedDataCache, ImportRequest, ImporterRegistry,
    PlaceholderArtifact, content_hash,
};
use engine_core::AssetId;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// Stale-safe asynchronous asset handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AssetHandle {
    /// Stable source identity.
    pub id: AssetId,
    /// Load/reimport generation.
    pub generation: u32,
}

/// Snapshot of asynchronous load state.
#[derive(Debug, Clone)]
pub enum AssetLoadState {
    /// Work is queued/running; placeholder is immediately usable.
    Loading(Arc<DerivedArtifact>),
    /// Imported or loaded from DDC.
    Ready(Arc<DerivedArtifact>),
    /// Import failed; last-good output or error placeholder remains usable.
    Error {
        /// Usable fallback.
        fallback: Arc<DerivedArtifact>,
        /// Actionable diagnostic.
        diagnostic: String,
    },
}

#[derive(Debug)]
struct Slot {
    generation: u32,
    state: AssetLoadState,
    executed_thread: Option<String>,
}

struct SharedState {
    slots: Mutex<BTreeMap<AssetId, Slot>>,
    changed: Condvar,
    imports_executed: AtomicU64,
}

struct Job {
    handle: AssetHandle,
    meta: AssetMeta,
    request: ImportRequest,
    dependency_hashes: Vec<String>,
}

enum WorkerCommand {
    Import(Box<Job>),
    Shutdown,
}

/// Queue saturation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportQueueOutcome {
    /// Accepted for background work.
    Queued(AssetHandle),
    /// Queue was full and no unbounded work was created.
    Full,
    /// Pipeline is shutting down.
    Stopped,
}

/// Bounded background import service.
pub struct AssetPipeline {
    sender: SyncSender<WorkerCommand>,
    shared: Arc<SharedState>,
    workers: Vec<JoinHandle<()>>,
    accepting: AtomicBool,
}

impl AssetPipeline {
    /// Starts a fixed-size worker pool.
    ///
    /// # Errors
    ///
    /// Returns an error when an operating-system worker thread cannot be created.
    pub fn start(
        worker_count: usize,
        queue_capacity: usize,
        ddc: &DerivedDataCache,
    ) -> Result<Self, crate::AssetError> {
        let (sender, receiver) = sync_channel(queue_capacity.max(1));
        let receiver = Arc::new(Mutex::new(receiver));
        let shared = Arc::new(SharedState {
            slots: Mutex::new(BTreeMap::new()),
            changed: Condvar::new(),
            imports_executed: AtomicU64::new(0),
        });
        let mut workers = Vec::new();
        for index in 0..worker_count.max(1) {
            let receiver = Arc::clone(&receiver);
            let shared = Arc::clone(&shared);
            let ddc = ddc.clone();
            let worker = thread::Builder::new()
                .name(format!("rustic-import-{index}"))
                .spawn(move || worker_loop(&receiver, &shared, &ddc))
                .map_err(|error| crate::AssetError::Worker(error.to_string()))?;
            workers.push(worker);
        }
        Ok(Self {
            sender,
            shared,
            workers,
            accepting: AtomicBool::new(true),
        })
    }

    /// Requests import/reimport without doing hashing, decoding, or I/O on the caller thread.
    pub fn request_import(
        &self,
        meta: AssetMeta,
        request: ImportRequest,
        dependency_hashes: Vec<String>,
    ) -> ImportQueueOutcome {
        if !self.accepting.load(Ordering::Acquire) {
            return ImportQueueOutcome::Stopped;
        }
        let kind = ImporterRegistry::contract(&request.source_path)
            .map_or(AssetKind::Texture, |contract| contract.kind);
        let mut slots = self
            .shared
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let previous = slots.get(&meta.asset_id);
        let generation = previous.map_or(1, |slot| slot.generation.wrapping_add(1).max(1));
        let fallback = previous.map(|slot| match &slot.state {
            AssetLoadState::Ready(value)
            | AssetLoadState::Loading(value)
            | AssetLoadState::Error {
                fallback: value, ..
            } => Arc::clone(value),
        });
        let fallback = fallback.unwrap_or_else(|| {
            Arc::new(DerivedArtifact::Placeholder(PlaceholderArtifact {
                asset_id: meta.asset_id,
                kind,
                reason: None,
            }))
        });
        let handle = AssetHandle {
            id: meta.asset_id,
            generation,
        };
        slots.insert(
            meta.asset_id,
            Slot {
                generation,
                state: AssetLoadState::Loading(fallback),
                executed_thread: None,
            },
        );
        drop(slots);
        match self.sender.try_send(WorkerCommand::Import(Box::new(Job {
            handle,
            meta,
            request,
            dependency_hashes,
        }))) {
            Ok(()) => ImportQueueOutcome::Queued(handle),
            Err(TrySendError::Full(_)) => {
                self.set_error(handle, "bounded import queue is full".to_owned());
                ImportQueueOutcome::Full
            }
            Err(TrySendError::Disconnected(_)) => ImportQueueOutcome::Stopped,
        }
    }

    /// Returns current state, rejecting stale generations.
    pub fn state(&self, handle: AssetHandle) -> Option<AssetLoadState> {
        self.shared
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&handle.id)
            .filter(|slot| slot.generation == handle.generation)
            .map(|slot| slot.state.clone())
    }

    /// Waits for loading to finish, with a bounded deadline.
    pub fn wait(&self, handle: AssetHandle, timeout: Duration) -> Option<AssetLoadState> {
        let deadline = Instant::now() + timeout;
        let mut slots = self
            .shared
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            let slot = slots
                .get(&handle.id)
                .filter(|slot| slot.generation == handle.generation)?;
            if !matches!(slot.state, AssetLoadState::Loading(_)) {
                return Some(slot.state.clone());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Some(slot.state.clone());
            }
            let (next, timeout_result) = self
                .shared
                .changed
                .wait_timeout(slots, remaining)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            slots = next;
            if timeout_result.timed_out() {
                return slots
                    .get(&handle.id)
                    .filter(|slot| slot.generation == handle.generation)
                    .map(|slot| slot.state.clone());
            }
        }
    }

    /// Number of decoder executions; DDC hits do not increment this counter.
    pub fn imports_executed(&self) -> u64 {
        self.shared.imports_executed.load(Ordering::Relaxed)
    }

    /// Worker thread name that completed a handle, for responsiveness/thread-affinity evidence.
    pub fn executed_thread(&self, handle: AssetHandle) -> Option<String> {
        self.shared
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&handle.id)
            .filter(|slot| slot.generation == handle.generation)
            .and_then(|slot| slot.executed_thread.clone())
    }

    fn set_error(&self, handle: AssetHandle, diagnostic: String) {
        let mut slots = self
            .shared
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slot) = slots
            .get_mut(&handle.id)
            .filter(|slot| slot.generation == handle.generation)
        {
            let fallback = match &slot.state {
                AssetLoadState::Loading(value)
                | AssetLoadState::Ready(value)
                | AssetLoadState::Error {
                    fallback: value, ..
                } => Arc::clone(value),
            };
            slot.state = AssetLoadState::Error {
                fallback,
                diagnostic,
            };
        }
        self.shared.changed.notify_all();
    }

    /// Stops after queued work and joins every worker.
    pub fn shutdown(&mut self) {
        if !self.accepting.swap(false, Ordering::AcqRel) {
            return;
        }
        for _ in &self.workers {
            let _ = self.sender.send(WorkerCommand::Shutdown);
        }
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

impl Drop for AssetPipeline {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn worker_loop(
    receiver: &Mutex<Receiver<WorkerCommand>>,
    shared: &SharedState,
    ddc: &DerivedDataCache,
) {
    loop {
        let command = receiver
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .recv();
        let Ok(command) = command else { break };
        let WorkerCommand::Import(job) = command else {
            break;
        };
        let source_hash = content_hash(&job.request.source_bytes);
        let recipe = job.meta.recipe_hash(&source_hash, &job.dependency_hashes);
        let result = if let Ok(artifact) = ddc.load(job.meta.asset_id, &recipe) {
            Ok(artifact)
        } else {
            shared.imports_executed.fetch_add(1, Ordering::Relaxed);
            ImporterRegistry::import(&job.request).and_then(|artifact| {
                ddc.store(job.meta.asset_id, &recipe, &artifact)?;
                Ok(artifact)
            })
        };
        let thread_name = thread::current().name().map(str::to_owned);
        let mut slots = shared
            .slots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slot) = slots
            .get_mut(&job.handle.id)
            .filter(|slot| slot.generation == job.handle.generation)
        {
            slot.executed_thread = thread_name;
            match result {
                Ok(artifact) => slot.state = AssetLoadState::Ready(Arc::new(artifact)),
                Err(error) => {
                    let fallback = match &slot.state {
                        AssetLoadState::Loading(value)
                        | AssetLoadState::Ready(value)
                        | AssetLoadState::Error {
                            fallback: value, ..
                        } => Arc::clone(value),
                    };
                    slot.state = AssetLoadState::Error {
                        fallback,
                        diagnostic: error.to_string(),
                    };
                }
            }
        }
        shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_runs_off_thread_and_unchanged_recipe_hits_ddc() {
        let directory = tempfile::tempdir().unwrap();
        let cache = DerivedDataCache::new(directory.path());
        let mut pipeline = AssetPipeline::start(1, 4, &cache).unwrap();
        let meta = AssetMeta::new("rustic.obj", 1);
        let request = ImportRequest::new(
            "triangle.obj",
            b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_vec(),
        );
        let ImportQueueOutcome::Queued(first) =
            pipeline.request_import(meta.clone(), request.clone(), vec![])
        else {
            panic!("queue")
        };
        assert!(matches!(
            pipeline.wait(first, Duration::from_secs(5)),
            Some(AssetLoadState::Ready(_))
        ));
        assert!(
            pipeline
                .executed_thread(first)
                .is_some_and(|name| name.starts_with("rustic-import-"))
        );
        assert_eq!(pipeline.imports_executed(), 1);

        let ImportQueueOutcome::Queued(second) = pipeline.request_import(meta, request, vec![])
        else {
            panic!("queue")
        };
        assert!(matches!(
            pipeline.wait(second, Duration::from_secs(5)),
            Some(AssetLoadState::Ready(_))
        ));
        assert_eq!(pipeline.imports_executed(), 1);
        pipeline.shutdown();
    }

    #[test]
    fn malformed_source_retains_typed_placeholder() {
        let directory = tempfile::tempdir().unwrap();
        let cache = DerivedDataCache::new(directory.path());
        let mut pipeline = AssetPipeline::start(1, 2, &cache).unwrap();
        let meta = AssetMeta::new("rustic.image", 1);
        let ImportQueueOutcome::Queued(handle) =
            pipeline.request_import(meta, ImportRequest::new("bad.png", vec![0, 1, 2]), vec![])
        else {
            panic!("queue")
        };
        let Some(AssetLoadState::Error { fallback, .. }) =
            pipeline.wait(handle, Duration::from_secs(5))
        else {
            panic!("expected error")
        };
        assert_eq!(fallback.kind(), AssetKind::Texture);
        pipeline.shutdown();
    }
}
