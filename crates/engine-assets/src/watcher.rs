//! Debounced polling watcher. Polling is the portable fallback when native notifications overflow.

use crate::{AssetError, io_error};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};

/// Coalesced source change kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    /// New file.
    Created,
    /// Existing file changed.
    Modified,
    /// Existing file disappeared.
    Removed,
    /// Watcher overflow/full rescan request.
    Rescan,
}

/// One stable source change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetChange {
    /// Asset-root-relative path.
    pub path: PathBuf,
    /// Coalesced kind.
    pub kind: ChangeKind,
}

/// Deterministic debounce model driven by caller-supplied monotonic milliseconds.
#[derive(Debug)]
pub struct ChangeDebouncer {
    delay_millis: u64,
    pending: BTreeMap<PathBuf, (u64, ChangeKind)>,
}

impl ChangeDebouncer {
    /// Creates a coalescer with a non-zero quiet period.
    pub fn new(delay: Duration) -> Self {
        Self {
            delay_millis: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX).max(1),
            pending: BTreeMap::new(),
        }
    }

    /// Records/replaces the pending event for a path.
    pub fn push(&mut self, now_millis: u64, change: AssetChange) {
        let deadline = now_millis.saturating_add(self.delay_millis);
        self.pending.insert(change.path, (deadline, change.kind));
    }

    /// Drains stable changes in deterministic path order.
    pub fn drain_ready(&mut self, now_millis: u64) -> Vec<AssetChange> {
        let paths = self
            .pending
            .iter()
            .filter_map(|(path, (deadline, _))| (*deadline <= now_millis).then_some(path.clone()))
            .collect::<Vec<_>>();
        paths
            .into_iter()
            .filter_map(|path| {
                self.pending
                    .remove(&path)
                    .map(|(_, kind)| AssetChange { path, kind })
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    length: u64,
}

/// Background watcher handle. Scanning and debounce never execute on the UI thread.
pub struct AssetWatcher {
    receiver: Receiver<Vec<AssetChange>>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl AssetWatcher {
    /// Starts a bounded polling watcher.
    ///
    /// # Errors
    ///
    /// Returns an error when the root cannot be canonicalized or the watcher thread cannot start.
    pub fn start(
        root: impl Into<PathBuf>,
        poll_interval: Duration,
        debounce: Duration,
        queue_capacity: usize,
    ) -> Result<Self, AssetError> {
        let root = root.into();
        let root = fs::canonicalize(&root).map_err(|error| io_error(&root, error))?;
        let (sender, receiver) = sync_channel(queue_capacity.max(1));
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("rustic-asset-watcher".to_owned())
            .spawn(move || {
                watcher_loop(
                    &root,
                    poll_interval.max(Duration::from_millis(10)),
                    debounce,
                    &sender,
                    &worker_cancelled,
                );
            })
            .map_err(|error| AssetError::Worker(error.to_string()))?;
        Ok(Self {
            receiver,
            cancelled,
            worker: Some(worker),
        })
    }

    /// Non-blocking retrieval of the next stable change batch.
    pub fn try_recv(&self) -> Option<Vec<AssetChange>> {
        self.receiver.try_recv().ok()
    }

    /// Stops and joins the watcher.
    pub fn shutdown(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for AssetWatcher {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn watcher_loop(
    root: &Path,
    poll_interval: Duration,
    debounce: Duration,
    sender: &SyncSender<Vec<AssetChange>>,
    cancelled: &AtomicBool,
) {
    let mut previous = scan_stamps(root).unwrap_or_default();
    let started = std::time::Instant::now();
    let mut debouncer = ChangeDebouncer::new(debounce);
    while !cancelled.load(Ordering::Acquire) {
        thread::sleep(poll_interval);
        let now = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match scan_stamps(root) {
            Ok(current) => {
                for (path, stamp) in &current {
                    let kind = match previous.get(path) {
                        None => Some(ChangeKind::Created),
                        Some(old) if old != stamp => Some(ChangeKind::Modified),
                        Some(_) => None,
                    };
                    if let Some(kind) = kind {
                        debouncer.push(
                            now,
                            AssetChange {
                                path: path.clone(),
                                kind,
                            },
                        );
                    }
                }
                for path in previous.keys().filter(|path| !current.contains_key(*path)) {
                    debouncer.push(
                        now,
                        AssetChange {
                            path: path.clone(),
                            kind: ChangeKind::Removed,
                        },
                    );
                }
                previous = current;
            }
            Err(_) => debouncer.push(
                now,
                AssetChange {
                    path: PathBuf::new(),
                    kind: ChangeKind::Rescan,
                },
            ),
        }
        let ready = debouncer.drain_ready(now);
        if !ready.is_empty() && sender.try_send(ready).is_err() {
            // A full consumer queue is itself an overflow: the next successful scan sends a
            // deterministic rescan request instead of an unbounded backlog.
            debouncer.push(
                now,
                AssetChange {
                    path: PathBuf::new(),
                    kind: ChangeKind::Rescan,
                },
            );
        }
    }
}

fn scan_stamps(root: &Path) -> Result<BTreeMap<PathBuf, Stamp>, AssetError> {
    let mut result = BTreeMap::new();
    let mut queue = vec![root.to_path_buf()];
    while let Some(directory) = queue.pop() {
        for entry in fs::read_dir(&directory).map_err(|error| io_error(&directory, error))? {
            let entry = entry.map_err(|error| io_error(&directory, error))?;
            let file_type = entry
                .file_type()
                .map_err(|error| io_error(entry.path(), error))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                queue.push(entry.path());
            } else if file_type.is_file() {
                let path = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| AssetError::UnsafePath(entry.path()))?
                    .to_path_buf();
                let metadata = entry
                    .metadata()
                    .map_err(|error| io_error(entry.path(), error))?;
                result.insert(
                    path,
                    Stamp {
                        modified: metadata.modified().ok(),
                        length: metadata.len(),
                    },
                );
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_coalesces_repeated_changes() {
        let mut debounce = ChangeDebouncer::new(Duration::from_millis(100));
        debounce.push(
            0,
            AssetChange {
                path: "a.png".into(),
                kind: ChangeKind::Created,
            },
        );
        debounce.push(
            50,
            AssetChange {
                path: "a.png".into(),
                kind: ChangeKind::Modified,
            },
        );
        assert!(debounce.drain_ready(149).is_empty());
        assert_eq!(
            debounce.drain_ready(150),
            vec![AssetChange {
                path: "a.png".into(),
                kind: ChangeKind::Modified,
            }]
        );
    }
}
