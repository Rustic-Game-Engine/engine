//! Small bounded CPU job scheduler with cancellation and priority lanes.

use crate::{CancellationSource, CancellationToken};
use crossbeam_channel::{Receiver, Sender, TrySendError, bounded, select_biased, unbounded};
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use thiserror::Error;

/// Scheduling priority. This is a hint; starvation-sensitive work needs a domain scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobPriority {
    /// Latency-sensitive user-visible work.
    High,
    /// Ordinary background work.
    Normal,
    /// Opportunistic maintenance.
    Low,
}

/// Failure returned by a submitted job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum JobError {
    /// Work was cancelled before it began.
    #[error("job was cancelled")]
    Cancelled,
    /// The job panicked; the worker remained alive.
    #[error("job panicked")]
    Panicked,
    /// The scheduler stopped before producing a result.
    #[error("job scheduler stopped")]
    SchedulerStopped,
}

/// Non-blocking submission failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SubmitError {
    /// The selected bounded priority lane is full.
    #[error("job queue is full")]
    QueueFull,
    /// Scheduler shutdown has begun.
    #[error("job scheduler is stopped")]
    Stopped,
}

/// Snapshot of scheduler load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JobSchedulerStats {
    /// Jobs waiting in all priority lanes.
    pub queued: usize,
    /// Jobs currently executing.
    pub active: usize,
    /// Jobs completed successfully.
    pub completed: u64,
    /// Jobs cancelled before execution.
    pub cancelled: u64,
    /// Panics contained at the job boundary.
    pub panicked: u64,
}

#[derive(Default)]
struct AtomicJobStats {
    queued: AtomicUsize,
    active: AtomicUsize,
    completed: AtomicU64,
    cancelled: AtomicU64,
    panicked: AtomicU64,
}

type Task = Box<dyn FnOnce() + Send + 'static>;

struct SchedulerInner {
    high: Sender<Task>,
    normal: Sender<Task>,
    low: Sender<Task>,
    shutdown: Sender<()>,
    stopped: AtomicBool,
    stats: Arc<AtomicJobStats>,
    workers: parking_lot::Mutex<Vec<JoinHandle<()>>>,
}

/// Bounded worker pool shared by launcher and foundation services.
#[derive(Clone)]
pub struct JobScheduler {
    inner: Arc<SchedulerInner>,
}

impl fmt::Debug for JobScheduler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JobScheduler")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl JobScheduler {
    /// Starts `worker_count` named workers and gives each priority a bounded lane.
    ///
    /// # Panics
    ///
    /// Panics when either argument is zero or an operating-system thread cannot start.
    pub fn new(worker_count: usize, lane_capacity: usize) -> Self {
        assert!(worker_count > 0, "worker_count must be non-zero");
        assert!(lane_capacity > 0, "lane_capacity must be non-zero");
        let (high_tx, high_rx) = bounded(lane_capacity);
        let (normal_tx, normal_rx) = bounded(lane_capacity);
        let (low_tx, low_rx) = bounded(lane_capacity);
        let (shutdown_tx, shutdown_rx) = unbounded();
        let stats = Arc::new(AtomicJobStats::default());
        let mut workers = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let high_rx = high_rx.clone();
            let normal_rx = normal_rx.clone();
            let low_rx = low_rx.clone();
            let shutdown_rx = shutdown_rx.clone();
            let worker_stats = Arc::clone(&stats);
            workers.push(
                thread::Builder::new()
                    .name(format!("rustic-job-{index}"))
                    .spawn(move || {
                        worker_loop(&high_rx, &normal_rx, &low_rx, &shutdown_rx, &worker_stats);
                    })
                    .expect("could not start Rustic job worker"),
            );
        }
        Self {
            inner: Arc::new(SchedulerInner {
                high: high_tx,
                normal: normal_tx,
                low: low_tx,
                shutdown: shutdown_tx,
                stopped: AtomicBool::new(false),
                stats,
                workers: parking_lot::Mutex::new(workers),
            }),
        }
    }

    /// Submits work without waiting for queue space.
    ///
    /// # Errors
    ///
    /// Returns [`SubmitError::QueueFull`] when bounded capacity is exhausted or
    /// [`SubmitError::Stopped`] after scheduler shutdown.
    pub fn submit<T, F>(&self, priority: JobPriority, work: F) -> Result<JobHandle<T>, SubmitError>
    where
        T: Send + 'static,
        F: FnOnce(CancellationToken) -> T + Send + 'static,
    {
        if self.inner.stopped.load(Ordering::Acquire) {
            return Err(SubmitError::Stopped);
        }
        let cancellation = CancellationSource::new();
        let token = cancellation.token();
        let task_token = token.clone();
        let (result_tx, result_rx) = bounded(1);
        let stats = Arc::clone(&self.inner.stats);
        let task: Task = Box::new(move || {
            if task_token.is_cancelled() {
                stats.cancelled.fetch_add(1, Ordering::Relaxed);
                let _ = result_tx.try_send(Err(JobError::Cancelled));
                return;
            }
            let result = catch_unwind(AssertUnwindSafe(|| work(task_token)));
            if let Ok(value) = result {
                stats.completed.fetch_add(1, Ordering::Relaxed);
                let _ = result_tx.try_send(Ok(value));
            } else {
                stats.panicked.fetch_add(1, Ordering::Relaxed);
                let _ = result_tx.try_send(Err(JobError::Panicked));
            }
        });
        let sender = match priority {
            JobPriority::High => &self.inner.high,
            JobPriority::Normal => &self.inner.normal,
            JobPriority::Low => &self.inner.low,
        };
        self.inner.stats.queued.fetch_add(1, Ordering::Relaxed);
        match sender.try_send(task) {
            Ok(()) => Ok(JobHandle {
                receiver: result_rx,
                cancellation,
            }),
            Err(TrySendError::Full(_)) => {
                self.inner.stats.queued.fetch_sub(1, Ordering::Relaxed);
                Err(SubmitError::QueueFull)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.inner.stats.queued.fetch_sub(1, Ordering::Relaxed);
                Err(SubmitError::Stopped)
            }
        }
    }

    /// Returns current queue and completion counters.
    pub fn stats(&self) -> JobSchedulerStats {
        JobSchedulerStats {
            queued: self.inner.stats.queued.load(Ordering::Relaxed),
            active: self.inner.stats.active.load(Ordering::Relaxed),
            completed: self.inner.stats.completed.load(Ordering::Relaxed),
            cancelled: self.inner.stats.cancelled.load(Ordering::Relaxed),
            panicked: self.inner.stats.panicked.load(Ordering::Relaxed),
        }
    }

    /// Stops workers after their current jobs. Repeated calls are harmless.
    pub fn shutdown(&self) {
        if self.inner.stopped.swap(true, Ordering::AcqRel) {
            return;
        }
        let count = self.inner.workers.lock().len();
        for _ in 0..count {
            let _ = self.inner.shutdown.send(());
        }
        for worker in self.inner.workers.lock().drain(..) {
            let _ = worker.join();
        }
    }
}

impl Drop for SchedulerInner {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let workers = self.workers.get_mut();
        for _ in 0..workers.len() {
            let _ = self.shutdown.send(());
        }
        for worker in workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    high: &Receiver<Task>,
    normal: &Receiver<Task>,
    low: &Receiver<Task>,
    shutdown: &Receiver<()>,
    stats: &AtomicJobStats,
) {
    loop {
        select_biased! {
            recv(shutdown) -> _ => break,
            recv(high) -> task => match task { Ok(task) => run_task(task, stats), Err(_) => break },
            recv(normal) -> task => match task { Ok(task) => run_task(task, stats), Err(_) => break },
            recv(low) -> task => match task { Ok(task) => run_task(task, stats), Err(_) => break },
        }
    }
}

fn run_task(task: Task, stats: &AtomicJobStats) {
    stats.queued.fetch_sub(1, Ordering::Relaxed);
    stats.active.fetch_add(1, Ordering::Relaxed);
    task();
    stats.active.fetch_sub(1, Ordering::Relaxed);
}

/// Result and cancellation handle for one submitted job.
pub struct JobHandle<T> {
    receiver: Receiver<Result<T, JobError>>,
    cancellation: CancellationSource,
}

impl<T> JobHandle<T> {
    /// Requests cancellation. Running work must observe its token cooperatively.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Waits for the job result.
    ///
    /// # Errors
    ///
    /// Returns cancellation, panic-containment, or scheduler-shutdown diagnostics.
    pub fn wait(self) -> Result<T, JobError> {
        self.receiver
            .recv()
            .unwrap_or(Err(JobError::SchedulerStopped))
    }

    /// Attempts to receive a completed result without blocking.
    pub fn try_result(&self) -> Option<Result<T, JobError>> {
        self.receiver.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[test]
    fn scheduler_returns_values_and_contains_panics() {
        let scheduler = JobScheduler::new(1, 4);
        assert_eq!(
            scheduler
                .submit(JobPriority::Normal, |_| 42)
                .unwrap()
                .wait(),
            Ok(42)
        );
        let panic = scheduler
            .submit(JobPriority::Normal, |_| -> () { panic!("fixture") })
            .unwrap()
            .wait();
        assert_eq!(panic, Err(JobError::Panicked));
        assert_eq!(scheduler.stats().panicked, 1);
        scheduler.shutdown();
    }

    #[test]
    fn queued_job_can_be_cancelled_before_execution() {
        let scheduler = JobScheduler::new(1, 2);
        let release = Arc::new(AtomicBool::new(false));
        let worker_release = Arc::clone(&release);
        let blocker = scheduler
            .submit(JobPriority::High, move |_| {
                while !worker_release.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
            })
            .unwrap();
        let cancelled = scheduler
            .submit(JobPriority::Normal, |_| 5)
            .expect("queue cancellation fixture");
        cancelled.cancel();
        release.store(true, Ordering::Release);
        blocker.wait().unwrap();
        assert_eq!(cancelled.wait(), Err(JobError::Cancelled));
        scheduler.shutdown();
    }

    #[test]
    fn cancellation_wait_is_bounded() {
        let source = CancellationSource::new();
        assert!(!source.token().wait_timeout(Duration::from_millis(1)));
    }
}
