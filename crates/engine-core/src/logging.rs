//! Structured, bounded, asynchronous logging for engine and project processes.
//!
//! Producers never perform file I/O. Normal log delivery is deliberately
//! non-blocking: when the bounded queue is full, records are dropped and the
//! worker writes a summary warning as soon as capacity becomes available.

use std::collections::{BTreeMap, HashSet};
use std::fmt::{self, Write as _};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{
    Receiver, RecvTimeoutError, SendTimeoutError, Sender, TryRecvError, TrySendError, bounded,
    select_biased,
};
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use time::OffsetDateTime;
use uuid::Uuid;

const DEFAULT_CHANNEL_CAPACITY: usize = 8_192;
const DEFAULT_SEVERE_CHANNEL_CAPACITY: usize = 256;
const DEFAULT_MAX_FILE_SIZE: u64 = 32 * 1024 * 1024;
const DEFAULT_RETAINED_FILES: usize = 10;
const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_CONTROL_TIMEOUT: Duration = Duration::from_secs(5);
const BUFFER_CAPACITY: usize = 64 * 1024;

/// Severity ordering is intentional and is used by [`LogFilter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Extremely detailed tracing intended for targeted diagnosis.
    Verbose,
    /// Developer-oriented diagnostic information.
    Debug,
    /// Normal lifecycle or operational information.
    Info,
    /// A recoverable condition that may need attention.
    Warning,
    /// An operation failed, but the process can continue.
    Error,
    /// An unrecoverable condition for the current process or session.
    Fatal,
}

impl Severity {
    /// All severity values in increasing order.
    pub const ALL: [Self; 6] = [
        Self::Verbose,
        Self::Debug,
        Self::Info,
        Self::Warning,
        Self::Error,
        Self::Fatal,
    ];

    /// Returns the stable uppercase name used in human-readable logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Verbose => "VERBOSE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warning => "WARNING",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        }
    }
}

impl fmt::Display for Severity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Severity {
    type Err = ParseSeverityError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "verbose" | "trace" => Ok(Self::Verbose),
            "debug" => Ok(Self::Debug),
            "info" => Ok(Self::Info),
            "warning" | "warn" => Ok(Self::Warning),
            "error" => Ok(Self::Error),
            "fatal" => Ok(Self::Fatal),
            _ => Err(ParseSeverityError(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown log severity '{0}'")]
/// Error returned when parsing an unknown [`Severity`] name.
pub struct ParseSeverityError(pub String);

/// A filterable engine subsystem/category. Custom subsystem names are allowed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Subsystem(String);

impl Subsystem {
    /// Creates a subsystem, substituting the default for a blank name.
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        if name.trim().is_empty() {
            Self::default()
        } else {
            Self(name)
        }
    }

    /// Returns the subsystem name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Subsystem {
    fn default() -> Self {
        Self("engine".to_owned())
    }
}

impl fmt::Display for Subsystem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl From<&str> for Subsystem {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Subsystem {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

/// Programming-language origin for script and compiler diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// Rust engine or game code.
    Rust,
    /// C source code.
    C,
    /// C++ source code.
    Cpp,
    /// C# source code.
    CSharp,
    /// Python source code.
    Python,
    /// JavaScript source code.
    JavaScript,
    /// Lua source code.
    Lua,
    /// Luau source code.
    Luau,
    /// Java source code.
    Java,
    /// PHP source code.
    Php,
    /// HTML documents or embedded scripts.
    Html,
    /// CSS stylesheets.
    Css,
    /// A runtime language supplied by a plugin.
    Other(String),
}

impl Language {
    /// Returns the conventional display name for the language.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Rust => "Rust",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::CSharp => "C#",
            Self::Python => "Python",
            Self::JavaScript => "JavaScript",
            Self::Lua => "Lua",
            Self::Luau => "Luau",
            Self::Java => "Java",
            Self::Php => "PHP",
            Self::Html => "HTML",
            Self::Css => "CSS",
            Self::Other(name) => name,
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Optional source location attached to a diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceLocation {
    /// Project-relative or absolute source filename.
    pub file: String,
    /// One-based line number, or zero when unavailable.
    pub line: u32,
    /// Optional one-based column number.
    pub column: Option<u32>,
    /// Optional function, method, or symbol name.
    pub function: Option<String>,
}

impl SourceLocation {
    /// Creates a location for a file and line.
    pub fn new(file: impl Into<String>, line: u32) -> Self {
        Self {
            file: file.into(),
            line,
            column: None,
            function: None,
        }
    }

    /// Adds a column number.
    #[must_use]
    pub fn with_column(mut self, column: u32) -> Self {
        self.column = Some(column);
        self
    }

    /// Adds a function or symbol name.
    #[must_use]
    pub fn with_function(mut self, function: impl Into<String>) -> Self {
        self.function = Some(function.into());
        self
    }
}

/// Captured producer-thread identity, useful when diagnosing concurrent systems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadContext {
    /// Debug representation of the Rust thread identifier.
    pub id: String,
    /// Thread name, when the producer thread has one.
    pub name: Option<String>,
}

impl ThreadContext {
    /// Captures the calling thread's identity.
    pub fn capture() -> Self {
        let current = thread::current();
        Self {
            id: format!("{:?}", current.id()),
            name: current.name().map(str::to_owned),
        }
    }
}

/// Structured metadata retained for the editor console and persistent logs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogMetadata {
    /// Engine system or user-defined category that emitted the record.
    pub subsystem: Subsystem,
    /// Script/runtime language associated with the record, if any.
    pub language: Option<Language>,
    /// Source location associated with the record, if any.
    pub source: Option<SourceLocation>,
    /// Producer thread identity, if capture was requested.
    pub thread: Option<ThreadContext>,
    /// Deterministically ordered structured key/value context.
    pub fields: BTreeMap<String, String>,
}

impl LogMetadata {
    /// Creates metadata for a subsystem.
    pub fn new(subsystem: impl Into<Subsystem>) -> Self {
        Self {
            subsystem: subsystem.into(),
            language: None,
            source: None,
            thread: None,
            fields: BTreeMap::new(),
        }
    }

    /// Associates a script/runtime language.
    #[must_use]
    pub fn with_language(mut self, language: Language) -> Self {
        self.language = Some(language);
        self
    }

    /// Associates a source location.
    #[must_use]
    pub fn with_source(mut self, source: SourceLocation) -> Self {
        self.source = Some(source);
        self
    }

    /// Associates an explicitly captured producer thread.
    #[must_use]
    pub fn with_thread(mut self, thread: ThreadContext) -> Self {
        self.thread = Some(thread);
        self
    }

    /// Adds a structured key/value field.
    #[must_use]
    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }
}

impl Default for LogMetadata {
    fn default() -> Self {
        Self::new(Subsystem::default())
    }
}

/// One serializable event. Nanoseconds since Unix epoch avoid imposing a
/// `time` serde format on consumers while retaining precise ordering data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogRecord {
    /// Globally unique event identifier.
    pub id: Uuid,
    /// UTC Unix timestamp with nanosecond precision.
    pub timestamp_unix_nanos: i128,
    /// Event severity.
    pub severity: Severity,
    /// Human-readable event message.
    pub message: String,
    /// Structured filtering and diagnostic metadata.
    pub metadata: LogMetadata,
}

impl LogRecord {
    /// Creates a record with a fresh identifier and current UTC timestamp.
    pub fn new(severity: Severity, message: impl Into<String>, metadata: LogMetadata) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp_unix_nanos: OffsetDateTime::now_utc().unix_timestamp_nanos(),
            severity,
            message: message.into(),
            metadata,
        }
    }

    /// Converts the stored timestamp to an [`OffsetDateTime`], if representable.
    pub fn timestamp(&self) -> Option<OffsetDateTime> {
        OffsetDateTime::from_unix_timestamp_nanos(self.timestamp_unix_nanos).ok()
    }
}

/// Runtime/editor-console filtering. `None` means all values in that dimension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogFilter {
    /// Lowest accepted severity.
    pub minimum_severity: Severity,
    /// Accepted subsystems, or every subsystem when absent.
    pub subsystems: Option<HashSet<Subsystem>>,
    /// Accepted languages, or every language when absent.
    pub languages: Option<HashSet<Language>>,
    /// Optional case-insensitive message substring.
    pub message_contains: Option<String>,
}

impl LogFilter {
    /// Returns whether a record passes every configured filter dimension.
    pub fn accepts(&self, record: &LogRecord) -> bool {
        if record.severity < self.minimum_severity {
            return false;
        }

        if let Some(subsystems) = &self.subsystems
            && !subsystems.contains(&record.metadata.subsystem)
        {
            return false;
        }

        if let Some(languages) = &self.languages {
            match record.metadata.language.as_ref() {
                Some(language) if languages.contains(language) => {}
                _ => return false,
            }
        }

        if let Some(needle) = self
            .message_contains
            .as_deref()
            .map(str::trim)
            .filter(|needle| !needle.is_empty())
            && !record
                .message
                .to_lowercase()
                .contains(&needle.to_lowercase())
        {
            return false;
        }

        true
    }
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            minimum_severity: Severity::Verbose,
            subsystems: None,
            languages: None,
            message_contains: None,
        }
    }
}

/// Selects the timestamped log name while preserving `latest.log`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogStream {
    /// A game or local-play runtime session.
    Runtime,
    /// An editor process session.
    Editor,
    /// A dedicated crash-report stream.
    Crash,
}

impl LogStream {
    const fn file_prefix(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Editor => "editor",
            Self::Crash => "crash",
        }
    }
}

/// File and queue policy for one asynchronous logger.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Destination directory, normally `<ProjectRoot>/logs`.
    pub directory: PathBuf,
    /// Process/session stream used in timestamped filenames.
    pub stream: LogStream,
    /// Maximum number of pending producer commands.
    pub channel_capacity: usize,
    /// Reserved queue capacity used only by Error and Fatal records.
    pub severe_channel_capacity: usize,
    /// Per-file rotation threshold. A single oversized record remains atomic.
    pub max_file_size_bytes: u64,
    /// Number of timestamped/rotated files retained in addition to the active file.
    pub retained_files: usize,
    /// Maximum time between buffered worker flushes.
    pub flush_interval: Duration,
    /// Maximum wait for explicit flush and shutdown commands.
    pub control_timeout: Duration,
    /// Whether convenience logging captures producer-thread identity.
    pub capture_thread_context: bool,
    /// Initial producer-side filter.
    pub filter: LogFilter,
}

impl LogConfig {
    /// Creates default runtime logging configuration beneath a project root.
    pub fn for_project(project_root: impl AsRef<Path>) -> Self {
        Self {
            directory: project_root.as_ref().join("logs"),
            ..Self::default()
        }
    }

    fn validate(&self) -> Result<(), LogError> {
        if self.channel_capacity == 0 {
            return Err(LogError::InvalidConfig("channel capacity must be non-zero"));
        }
        if self.severe_channel_capacity == 0 {
            return Err(LogError::InvalidConfig(
                "severe channel capacity must be non-zero",
            ));
        }
        if self.max_file_size_bytes == 0 {
            return Err(LogError::InvalidConfig(
                "maximum file size must be non-zero",
            ));
        }
        if self.flush_interval.is_zero() {
            return Err(LogError::InvalidConfig("flush interval must be non-zero"));
        }
        if self.control_timeout.is_zero() {
            return Err(LogError::InvalidConfig("control timeout must be non-zero"));
        }
        Ok(())
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("logs"),
            stream: LogStream::Runtime,
            channel_capacity: DEFAULT_CHANNEL_CAPACITY,
            severe_channel_capacity: DEFAULT_SEVERE_CHANNEL_CAPACITY,
            max_file_size_bytes: DEFAULT_MAX_FILE_SIZE,
            retained_files: DEFAULT_RETAINED_FILES,
            flush_interval: DEFAULT_FLUSH_INTERVAL,
            control_timeout: DEFAULT_CONTROL_TIMEOUT,
            capture_thread_context: true,
            filter: LogFilter::default(),
        }
    }
}

/// Errors from logger startup or explicit lifecycle control operations.
#[derive(Debug, Error)]
pub enum LogError {
    /// A configuration invariant was violated.
    #[error("invalid logger configuration: {0}")]
    InvalidConfig(&'static str),
    /// Creating or initializing log output failed.
    #[error("log file I/O failed: {0}")]
    Io(#[from] io::Error),
    /// The background worker is stopped or disconnected.
    #[error("logging worker is unavailable")]
    WorkerUnavailable,
    /// A bounded lifecycle operation exceeded its configured deadline.
    #[error("logging control operation timed out")]
    TimedOut,
    /// The worker could not flush one or more output files.
    #[error("logging worker reported an I/O failure: {0}")]
    WorkerIo(String),
    /// The worker terminated because of a panic.
    #[error("logging worker panicked")]
    WorkerPanicked,
}

/// Non-blocking delivery result for ordinary records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogOutcome {
    /// The worker queue accepted the record.
    Enqueued,
    /// The record was accepted after evicting the oldest record in its lane.
    EnqueuedAfterDroppingOldest,
    /// Producer-side filtering rejected the record.
    Filtered,
    /// The bounded worker queue had no available capacity.
    DroppedQueueFull,
    /// The logger has shut down or the worker disconnected.
    WorkerUnavailable,
}

/// Snapshot of asynchronous logger health and delivery counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LoggerStats {
    /// User records accepted by the worker queue.
    pub enqueued: u64,
    /// Records written to at least one output, including drop summaries.
    pub written: u64,
    /// User records rejected by the current filter.
    pub filtered: u64,
    /// User records dropped because the queue was full.
    pub dropped: u64,
    /// Non-severe records evicted from the ordinary lane.
    pub dropped_low_severity: u64,
    /// Severe records evicted only after the reserved lane itself filled.
    pub dropped_severe: u64,
    /// Live-subscriber deliveries dropped because a subscriber was full.
    pub live_subscriber_dropped: u64,
    /// File write, rotation, cleanup, or flush failures observed by the worker.
    pub io_errors: u64,
    /// Panics contained at the worker thread boundary.
    pub worker_panics: u64,
    /// Whether the background worker is currently alive.
    pub worker_running: bool,
    /// Whether at least one persistent file sink is currently usable.
    pub sink_available: bool,
}

#[derive(Default)]
struct AtomicStats {
    enqueued: AtomicU64,
    written: AtomicU64,
    filtered: AtomicU64,
    dropped: AtomicU64,
    dropped_low_severity: AtomicU64,
    dropped_severe: AtomicU64,
    live_subscriber_dropped: AtomicU64,
    io_errors: AtomicU64,
    worker_panics: AtomicU64,
    worker_running: AtomicBool,
    sink_available: AtomicBool,
}

impl AtomicStats {
    fn snapshot(&self) -> LoggerStats {
        LoggerStats {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            written: self.written.load(Ordering::Relaxed),
            filtered: self.filtered.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            dropped_low_severity: self.dropped_low_severity.load(Ordering::Relaxed),
            dropped_severe: self.dropped_severe.load(Ordering::Relaxed),
            live_subscriber_dropped: self.live_subscriber_dropped.load(Ordering::Relaxed),
            io_errors: self.io_errors.load(Ordering::Relaxed),
            worker_panics: self.worker_panics.load(Ordering::Relaxed),
            worker_running: self.worker_running.load(Ordering::Acquire),
            sink_available: self.sink_available.load(Ordering::Acquire),
        }
    }
}

type WorkerResult = Result<(), String>;

enum Command {
    Flush(Sender<WorkerResult>),
    Shutdown(Sender<WorkerResult>),
}

struct LoggerInner {
    control_sender: Sender<Command>,
    normal_sender: Sender<Box<LogRecord>>,
    normal_receiver: Receiver<Box<LogRecord>>,
    severe_sender: Sender<Box<LogRecord>>,
    severe_receiver: Receiver<Box<LogRecord>>,
    filter: RwLock<LogFilter>,
    accepting: AtomicBool,
    pending_dropped: Arc<AtomicU64>,
    stats: Arc<AtomicStats>,
    capture_thread_context: bool,
    control_timeout: Duration,
    control_lock: Mutex<()>,
    worker: Mutex<Option<JoinHandle<()>>>,
    subscribers: Mutex<Vec<Sender<LogRecord>>>,
    latest_path: PathBuf,
    session_path: PathBuf,
}

/// Cloneable producer handle backed by a single bounded worker queue.
#[derive(Clone)]
pub struct AsyncLogger {
    inner: Arc<LoggerInner>,
}

impl AsyncLogger {
    /// Initializes output files and starts a named writer thread.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::InvalidConfig`] for invalid limits or
    /// [`LogError::Io`] when the output directory/files or worker thread cannot
    /// be initialized.
    pub fn start(config: LogConfig) -> Result<Self, LogError> {
        Self::start_internal(config, false)
    }

    /// Starts a logger whose live subscribers remain available when file startup fails.
    /// The failure is counted and reported to stderr instead of aborting the process.
    ///
    /// # Errors
    ///
    /// Returns configuration or worker-thread startup errors. File-sink errors are
    /// contained and exposed through [`LoggerStats::sink_available`].
    pub fn start_resilient(config: LogConfig) -> Result<Self, LogError> {
        Self::start_internal(config, true)
    }

    fn start_internal(config: LogConfig, resilient: bool) -> Result<Self, LogError> {
        config.validate()?;

        let (files, latest_path, session_path, initial_sink_error) = match LogFiles::open(&config) {
            Ok(files) => {
                let latest_path = files.latest.path.clone();
                let session_path = files.session.path.clone();
                (Some(files), latest_path, session_path, None)
            }
            Err(error) if resilient => {
                eprintln!("Rustic logging file sink unavailable: {error}");
                let latest_path = config.directory.join("latest.log");
                let session_path = config
                    .directory
                    .join(format!("{}-unavailable.log", config.stream.file_prefix()));
                (None, latest_path, session_path, Some(error))
            }
            Err(error) => return Err(LogError::Io(error)),
        };
        let (control_sender, control_receiver) = bounded(16);
        let (normal_sender, normal_receiver) = bounded(config.channel_capacity);
        let (severe_sender, severe_receiver) = bounded(config.severe_channel_capacity);
        let stats = Arc::new(AtomicStats::default());
        stats.worker_running.store(true, Ordering::Release);
        stats
            .sink_available
            .store(initial_sink_error.is_none(), Ordering::Release);
        if initial_sink_error.is_some() {
            stats.io_errors.fetch_add(1, Ordering::Relaxed);
        }
        let pending_dropped = Arc::new(AtomicU64::new(0));

        let worker_stats = Arc::clone(&stats);
        let worker_pending_dropped = Arc::clone(&pending_dropped);
        let flush_interval = config.flush_interval;
        let worker_normal = normal_receiver.clone();
        let worker_severe = severe_receiver.clone();
        let worker = thread::Builder::new()
            .name(format!("{}-log-writer", config.stream.file_prefix()))
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker_loop(
                        &control_receiver,
                        &worker_normal,
                        &worker_severe,
                        files,
                        flush_interval,
                        &worker_stats,
                        &worker_pending_dropped,
                    );
                }));
                if result.is_err() {
                    worker_stats.worker_panics.fetch_add(1, Ordering::Relaxed);
                }
                worker_stats.worker_running.store(false, Ordering::Release);
            })?;

        Ok(Self {
            inner: Arc::new(LoggerInner {
                control_sender,
                normal_sender,
                normal_receiver,
                severe_sender,
                severe_receiver,
                filter: RwLock::new(config.filter),
                accepting: AtomicBool::new(true),
                pending_dropped,
                stats,
                capture_thread_context: config.capture_thread_context,
                control_timeout: config.control_timeout,
                control_lock: Mutex::new(()),
                worker: Mutex::new(Some(worker)),
                subscribers: Mutex::new(Vec::new()),
                latest_path,
                session_path,
            }),
        })
    }

    /// Alias for [`AsyncLogger::start`].
    ///
    /// # Errors
    ///
    /// Returns the same configuration, I/O, or thread-start errors as
    /// [`AsyncLogger::start`].
    pub fn new(config: LogConfig) -> Result<Self, LogError> {
        Self::start(config)
    }

    /// Returns the path to the bounded current-session mirror.
    pub fn latest_log_path(&self) -> &Path {
        &self.inner.latest_path
    }

    /// Returns the path to the active timestamped session log.
    pub fn session_log_path(&self) -> &Path {
        &self.inner.session_path
    }

    /// Returns a snapshot of the producer-side filter.
    pub fn filter(&self) -> LogFilter {
        self.inner.filter.read().clone()
    }

    /// Replaces the producer-side filter for subsequent records.
    pub fn set_filter(&self, filter: LogFilter) {
        *self.inner.filter.write() = filter;
    }

    /// Tests a complete record against the current producer-side filter.
    pub fn is_enabled(&self, record: &LogRecord) -> bool {
        self.inner.filter.read().accepts(record)
    }

    /// Subscribes to accepted live records through a bounded non-blocking stream.
    ///
    /// # Errors
    ///
    /// Returns [`LogError::InvalidConfig`] when `capacity` is zero.
    pub fn subscribe(&self, capacity: usize) -> Result<Receiver<LogRecord>, LogError> {
        if capacity == 0 {
            return Err(LogError::InvalidConfig(
                "live subscriber capacity must be non-zero",
            ));
        }
        let (sender, receiver) = bounded(capacity);
        self.inner.subscribers.lock().push(sender);
        Ok(receiver)
    }

    /// Creates and attempts to enqueue a record using common metadata.
    pub fn log(
        &self,
        severity: Severity,
        subsystem: impl Into<Subsystem>,
        language: Option<Language>,
        message: impl Into<String>,
    ) -> LogOutcome {
        let mut metadata = LogMetadata::new(subsystem);
        metadata.language = language;
        if self.inner.capture_thread_context {
            metadata.thread = Some(ThreadContext::capture());
        }
        self.log_record(&LogRecord::new(severity, message, metadata))
    }

    /// Attempts non-blocking delivery. Queue pressure never stalls a gameplay thread.
    pub fn log_record(&self, record: &LogRecord) -> LogOutcome {
        if !self.inner.accepting.load(Ordering::Acquire) {
            return LogOutcome::WorkerUnavailable;
        }
        if !self.inner.filter.read().accepts(record) {
            self.inner.stats.filtered.fetch_add(1, Ordering::Relaxed);
            return LogOutcome::Filtered;
        }

        let severe = record.severity >= Severity::Error;
        let boxed = Box::new(record.clone());
        let (sender, receiver) = if severe {
            (&self.inner.severe_sender, &self.inner.severe_receiver)
        } else {
            (&self.inner.normal_sender, &self.inner.normal_receiver)
        };
        let outcome = match sender.try_send(boxed) {
            Ok(()) => {
                self.inner.stats.enqueued.fetch_add(1, Ordering::Relaxed);
                LogOutcome::Enqueued
            }
            Err(TrySendError::Full(boxed)) => {
                match receiver.try_recv() {
                    Ok(_) => {
                        self.record_queue_drop(severe);
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => {
                        self.inner.accepting.store(false, Ordering::Release);
                        return LogOutcome::WorkerUnavailable;
                    }
                }
                match sender.try_send(boxed) {
                    Ok(()) => {
                        self.inner.stats.enqueued.fetch_add(1, Ordering::Relaxed);
                        LogOutcome::EnqueuedAfterDroppingOldest
                    }
                    Err(TrySendError::Full(_)) => {
                        self.record_queue_drop(severe);
                        LogOutcome::DroppedQueueFull
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        self.inner.accepting.store(false, Ordering::Release);
                        LogOutcome::WorkerUnavailable
                    }
                }
            }
            Err(TrySendError::Disconnected(_)) => {
                self.inner.accepting.store(false, Ordering::Release);
                LogOutcome::WorkerUnavailable
            }
        };
        if matches!(
            outcome,
            LogOutcome::Enqueued | LogOutcome::EnqueuedAfterDroppingOldest
        ) {
            self.publish_live(record);
        }
        outcome
    }

    fn record_queue_drop(&self, severe: bool) {
        self.inner.pending_dropped.fetch_add(1, Ordering::Relaxed);
        self.inner.stats.dropped.fetch_add(1, Ordering::Relaxed);
        if severe {
            self.inner
                .stats
                .dropped_severe
                .fetch_add(1, Ordering::Relaxed);
        } else {
            self.inner
                .stats
                .dropped_low_severity
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    fn publish_live(&self, record: &LogRecord) {
        let mut subscribers = self.inner.subscribers.lock();
        subscribers.retain(|subscriber| match subscriber.try_send(record.clone()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.inner
                    .stats
                    .live_subscriber_dropped
                    .fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
        });
    }

    /// Flushes records accepted before this command to the operating system.
    ///
    /// # Errors
    ///
    /// Returns an availability, timeout, or worker I/O error when the bounded
    /// control command cannot complete.
    pub fn flush(&self) -> Result<(), LogError> {
        let _control = self.inner.control_lock.lock();
        if !self.inner.accepting.load(Ordering::Acquire) {
            return Err(LogError::WorkerUnavailable);
        }
        let (reply_tx, reply_rx) = bounded(1);
        send_control(
            &self.inner.control_sender,
            Command::Flush(reply_tx),
            self.inner.control_timeout,
        )?;
        receive_worker_result(&reply_rx, self.inner.control_timeout)
    }

    /// Stops the worker after draining commands queued before shutdown.
    /// Calling this method more than once is harmless.
    ///
    /// # Errors
    ///
    /// Returns an availability, timeout, worker I/O, or contained panic error
    /// if orderly shutdown cannot be confirmed.
    pub fn shutdown(&self) -> Result<(), LogError> {
        let _control = self.inner.control_lock.lock();
        if self.inner.worker.lock().is_none() {
            return Ok(());
        }

        self.inner.accepting.store(false, Ordering::Release);
        let (reply_tx, reply_rx) = bounded(1);
        let send_result = send_control(
            &self.inner.control_sender,
            Command::Shutdown(reply_tx),
            self.inner.control_timeout,
        );

        let worker_result = match send_result {
            Ok(()) => receive_worker_result(&reply_rx, self.inner.control_timeout),
            Err(error) => Err(error),
        };

        // A reply is sent immediately before worker exit, so joining cannot wait
        // on routine file I/O after a successful acknowledgement.
        if worker_result.is_ok() || matches!(worker_result, Err(LogError::WorkerUnavailable)) {
            let join_result = self.inner.worker.lock().take().map(JoinHandle::join);
            if matches!(join_result, Some(Err(_))) {
                return Err(LogError::WorkerPanicked);
            }
        }
        worker_result
    }

    /// Returns a snapshot of counters and worker health.
    pub fn stats(&self) -> LoggerStats {
        self.inner.stats.snapshot()
    }
}

impl Drop for LoggerInner {
    fn drop(&mut self) {
        self.accepting.store(false, Ordering::Release);
        let Some(worker) = self.worker.get_mut().take() else {
            return;
        };

        let (reply_tx, reply_rx) = bounded(1);
        // Drop must not hang process teardown. If the queue/worker is unhealthy,
        // dropping JoinHandle safely detaches it; sender disconnection then asks
        // the receive loop to finish.
        if self
            .control_sender
            .send_timeout(Command::Shutdown(reply_tx), Duration::from_millis(100))
            .is_ok()
            && reply_rx.recv_timeout(Duration::from_secs(1)).is_ok()
        {
            let _ = worker.join();
        }
    }
}

fn send_control(
    sender: &Sender<Command>,
    command: Command,
    timeout: Duration,
) -> Result<(), LogError> {
    match sender.send_timeout(command, timeout) {
        Ok(()) => Ok(()),
        Err(SendTimeoutError::Timeout(_)) => Err(LogError::TimedOut),
        Err(SendTimeoutError::Disconnected(_)) => Err(LogError::WorkerUnavailable),
    }
}

fn receive_worker_result(
    receiver: &Receiver<WorkerResult>,
    timeout: Duration,
) -> Result<(), LogError> {
    match receiver.recv_timeout(timeout) {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(LogError::WorkerIo(error)),
        Err(RecvTimeoutError::Timeout) => Err(LogError::TimedOut),
        Err(RecvTimeoutError::Disconnected) => Err(LogError::WorkerUnavailable),
    }
}

fn worker_loop(
    control: &Receiver<Command>,
    normal: &Receiver<Box<LogRecord>>,
    severe: &Receiver<Box<LogRecord>>,
    mut files: Option<LogFiles>,
    flush_interval: Duration,
    stats: &AtomicStats,
    pending_dropped: &AtomicU64,
) {
    let mut last_flush = Instant::now();
    loop {
        select_biased! {
            recv(control) -> command => match command {
                Ok(Command::Flush(reply)) => {
                    drain_records(normal, severe, &mut files, stats, pending_dropped);
                    let result = flush_files(&mut files, stats).map_err(|error| error.to_string());
                    let _ = reply.try_send(result);
                    last_flush = Instant::now();
                }
                Ok(Command::Shutdown(reply)) => {
                    drain_records(normal, severe, &mut files, stats, pending_dropped);
                    let result = flush_files(&mut files, stats).map_err(|error| error.to_string());
                    let _ = reply.try_send(result);
                    break;
                }
                Err(_) => {
                    drain_records(normal, severe, &mut files, stats, pending_dropped);
                    let _ = flush_files(&mut files, stats);
                    break;
                }
            },
            recv(severe) -> record => if let Ok(record) = record {
                write_queued_record(&mut files, &record, stats, pending_dropped);
            },
            recv(normal) -> record => if let Ok(record) = record {
                write_queued_record(&mut files, &record, stats, pending_dropped);
            },
            default(flush_interval) => {
                let _ = flush_files(&mut files, stats);
                last_flush = Instant::now();
            }
        }

        // Sustained traffic may prevent recv_timeout from expiring.
        if last_flush.elapsed() >= flush_interval {
            let _ = flush_files(&mut files, stats);
            last_flush = Instant::now();
        }
    }
}

fn drain_records(
    normal: &Receiver<Box<LogRecord>>,
    severe: &Receiver<Box<LogRecord>>,
    files: &mut Option<LogFiles>,
    stats: &AtomicStats,
    pending_dropped: &AtomicU64,
) {
    while let Ok(record) = severe.try_recv() {
        write_queued_record(files, &record, stats, pending_dropped);
    }
    while let Ok(record) = normal.try_recv() {
        write_queued_record(files, &record, stats, pending_dropped);
    }
}

fn write_queued_record(
    files: &mut Option<LogFiles>,
    record: &LogRecord,
    stats: &AtomicStats,
    pending_dropped: &AtomicU64,
) {
    let dropped_before = pending_dropped.swap(0, Ordering::AcqRel);
    if dropped_before > 0 {
        let warning = LogRecord::new(
            Severity::Warning,
            format!("logging queues overflowed; {dropped_before} record(s) were dropped"),
            LogMetadata::new("logging").with_field("dropped_records", dropped_before.to_string()),
        );
        write_record(files, &warning, stats);
    }
    write_record(files, record, stats);
}

fn write_record(files: &mut Option<LogFiles>, record: &LogRecord, stats: &AtomicStats) {
    let line = format_record(record);
    let bytes = line.as_bytes();
    let mut wrote_any = false;

    let Some(files) = files.as_mut() else {
        eprint!("{line}");
        stats.sink_available.store(false, Ordering::Release);
        return;
    };
    match files.latest.write_line(bytes) {
        Ok(()) => wrote_any = true,
        Err(_) => {
            stats.io_errors.fetch_add(1, Ordering::Relaxed);
        }
    }
    match files.session.write_line(bytes) {
        Ok(rotated) => {
            wrote_any = true;
            if rotated
                && prune_archives(
                    &files.directory,
                    files.stream,
                    files.retained_files,
                    &files.session.path,
                )
                .is_err()
            {
                stats.io_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
        Err(_) => {
            stats.io_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    if wrote_any {
        stats.written.fetch_add(1, Ordering::Relaxed);
        stats.sink_available.store(true, Ordering::Release);
    } else {
        eprint!("{line}");
        stats.sink_available.store(false, Ordering::Release);
    }
}

fn flush_files(files: &mut Option<LogFiles>, stats: &AtomicStats) -> io::Result<()> {
    let Some(files) = files.as_mut() else {
        return Ok(());
    };
    let latest_result = files.latest.flush();
    let session_result = files.session.flush();
    if latest_result.is_err() {
        stats.io_errors.fetch_add(1, Ordering::Relaxed);
    }
    if session_result.is_err() {
        stats.io_errors.fetch_add(1, Ordering::Relaxed);
    }
    latest_result.and(session_result)
}

struct LogFiles {
    directory: PathBuf,
    stream: LogStream,
    retained_files: usize,
    latest: LatestFile,
    session: SessionFile,
}

impl LogFiles {
    fn open(config: &LogConfig) -> io::Result<Self> {
        fs::create_dir_all(&config.directory)?;
        let latest = LatestFile::open(
            config.directory.join("latest.log"),
            config.max_file_size_bytes,
        )?;
        let session =
            SessionFile::create(&config.directory, config.stream, config.max_file_size_bytes)?;
        let files = Self {
            directory: config.directory.clone(),
            stream: config.stream,
            retained_files: config.retained_files,
            latest,
            session,
        };
        // Retention cleanup is intentionally best-effort. Inability to inspect an
        // old file must not prevent a new game/editor process from logging.
        let _ = prune_archives(
            &files.directory,
            files.stream,
            files.retained_files,
            &files.session.path,
        );
        Ok(files)
    }
}

struct LatestFile {
    path: PathBuf,
    writer: BufWriter<File>,
    size: u64,
    max_size: u64,
}

impl LatestFile {
    fn open(path: PathBuf, max_size: u64) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self {
            path,
            writer: BufWriter::with_capacity(BUFFER_CAPACITY, file),
            size: 0,
            max_size,
        })
    }

    fn write_line(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.size > 0 && self.size.saturating_add(bytes.len() as u64) > self.max_size {
            self.writer.flush()?;
            self.writer = BufWriter::with_capacity(
                BUFFER_CAPACITY,
                OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&self.path)?,
            );
            self.size = 0;
        }
        self.writer.write_all(bytes)?;
        self.size = self.size.saturating_add(bytes.len() as u64);
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

struct SessionFile {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
    size: u64,
    max_size: u64,
    next_part: u32,
}

impl SessionFile {
    fn create(directory: &Path, stream: LogStream, max_size: u64) -> io::Result<Self> {
        let timestamp = filename_timestamp(OffsetDateTime::now_utc());
        let stem = format!("{}-{timestamp}", stream.file_prefix());
        for collision in 0..10_000_u32 {
            let filename = if collision == 0 {
                format!("{stem}.log")
            } else {
                format!("{stem}-{collision:03}.log")
            };
            let path = directory.join(filename);
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => {
                    return Ok(Self {
                        path,
                        writer: Some(BufWriter::with_capacity(BUFFER_CAPACITY, file)),
                        size: 0,
                        max_size,
                        next_part: 1,
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate a unique timestamped log filename",
        ))
    }

    /// Returns whether this write caused a rotation.
    fn write_line(&mut self, bytes: &[u8]) -> io::Result<bool> {
        let mut rotated = false;
        if self.size > 0 && self.size.saturating_add(bytes.len() as u64) > self.max_size {
            self.rotate()?;
            rotated = true;
        }
        self.ensure_writer()?;
        if let Some(writer) = self.writer.as_mut() {
            writer.write_all(bytes)?;
            self.size = self.size.saturating_add(bytes.len() as u64);
        }
        Ok(rotated)
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut writer) = self.writer.take() {
            writer.flush()?;
            drop(writer);
        }

        let archive = loop {
            let candidate = part_path(&self.path, self.next_part);
            self.next_part = self.next_part.saturating_add(1);
            if !candidate.exists() {
                break candidate;
            }
        };

        if let Err(error) = fs::rename(&self.path, &archive) {
            // Recover append capability before returning the rotation error.
            let _ = self.ensure_writer();
            return Err(error);
        }

        match OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.path)
        {
            Ok(file) => {
                self.writer = Some(BufWriter::with_capacity(BUFFER_CAPACITY, file));
                self.size = 0;
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn ensure_writer(&mut self) -> io::Result<()> {
        if self.writer.is_none() {
            let file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?;
            self.size = file.metadata().map_or(0, |metadata| metadata.len());
            self.writer = Some(BufWriter::with_capacity(BUFFER_CAPACITY, file));
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.writer.as_mut() {
            Some(writer) => writer.flush(),
            None => Ok(()),
        }
    }
}

fn part_path(path: &Path, part: u32) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("runtime");
    path.with_file_name(format!("{stem}.part-{part:04}.log"))
}

fn prune_archives(
    directory: &Path,
    stream: LogStream,
    retained_files: usize,
    active: &Path,
) -> io::Result<()> {
    let prefix = format!("{}-", stream.file_prefix());
    let mut candidates = Vec::new();
    for entry in fs::read_dir(directory)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path == active || !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(&prefix)
            || !path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("log"))
        {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        candidates.push((modified, path));
    }

    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let remove_count = candidates.len().saturating_sub(retained_files);
    for (_, path) in candidates.into_iter().take(remove_count) {
        // A locked or permission-protected old log should not block cleanup of
        // other candidates or stop current logging.
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn filename_timestamp(timestamp: OffsetDateTime) -> String {
    let month: u8 = timestamp.month().into();
    format!(
        "{:04}-{:02}-{:02}-{:02}-{:02}-{:02}",
        timestamp.year(),
        month,
        timestamp.day(),
        timestamp.hour(),
        timestamp.minute(),
        timestamp.second()
    )
}

fn display_timestamp(timestamp_unix_nanos: i128) -> String {
    let timestamp = OffsetDateTime::from_unix_timestamp_nanos(timestamp_unix_nanos)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);
    let month: u8 = timestamp.month().into();
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        timestamp.year(),
        month,
        timestamp.day(),
        timestamp.hour(),
        timestamp.minute(),
        timestamp.second(),
        timestamp.millisecond()
    )
}

fn format_record(record: &LogRecord) -> String {
    let mut output = String::with_capacity(record.message.len().saturating_add(160));
    let _ = write!(
        output,
        "[{}] [{:<7}] [{}]",
        display_timestamp(record.timestamp_unix_nanos),
        record.severity,
        sanitize(record.metadata.subsystem.as_str())
    );
    if let Some(language) = &record.metadata.language {
        let _ = write!(output, " [{}]", sanitize(language.as_str()));
    }
    if let Some(thread) = &record.metadata.thread {
        if let Some(name) = &thread.name {
            let _ = write!(
                output,
                " [thread={}:{}]",
                sanitize(name),
                sanitize(&thread.id)
            );
        } else {
            let _ = write!(output, " [thread={}]", sanitize(&thread.id));
        }
    }
    let _ = write!(output, " [id={}] {}", record.id, sanitize(&record.message));
    if let Some(source) = &record.metadata.source {
        let _ = write!(output, " ({}:{}", sanitize(&source.file), source.line);
        if let Some(column) = source.column {
            let _ = write!(output, ":{column}");
        }
        if let Some(function) = &source.function {
            let _ = write!(output, " in {}", sanitize(function));
        }
        output.push(')');
    }
    for (key, value) in &record.metadata.fields {
        let _ = write!(output, " {}={}", sanitize(key), sanitize(value));
    }
    output.push('\n');
    output
}

fn sanitize(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\n' => sanitized.push_str("\\n"),
            '\r' => sanitized.push_str("\\r"),
            '\t' => sanitized.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(sanitized, "\\u{{{:x}}}", character as u32);
            }
            character => sanitized.push(character),
        }
    }
    sanitized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(directory: &Path) -> LogConfig {
        LogConfig {
            directory: directory.to_owned(),
            channel_capacity: 1_024,
            max_file_size_bytes: 1024 * 1024,
            retained_files: 4,
            flush_interval: Duration::from_millis(10),
            control_timeout: Duration::from_secs(2),
            capture_thread_context: false,
            ..LogConfig::default()
        }
    }

    #[test]
    fn filter_combines_severity_subsystem_language_and_text() {
        let record = LogRecord::new(
            Severity::Warning,
            "Shader compilation failed",
            LogMetadata::new("rendering").with_language(Language::JavaScript),
        );
        let filter = LogFilter {
            minimum_severity: Severity::Info,
            subsystems: Some(HashSet::from([Subsystem::new("rendering")])),
            languages: Some(HashSet::from([Language::JavaScript])),
            message_contains: Some("COMPILATION".to_owned()),
        };
        assert!(filter.accepts(&record));

        let mut rejected = filter;
        rejected.minimum_severity = Severity::Error;
        assert!(!rejected.accepts(&record));
    }

    #[test]
    fn writes_human_readable_latest_and_timestamped_session_logs() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let logger = AsyncLogger::start(test_config(directory.path())).expect("start logger");
        let session_path = logger.session_log_path().to_owned();

        let record = LogRecord::new(
            Severity::Error,
            "script failed\nwithout crashing the engine",
            LogMetadata::new("scripting")
                .with_language(Language::Python)
                .with_source(SourceLocation::new("scripts/player.py", 42).with_column(7))
                .with_field("entity", "player-1"),
        );
        assert_eq!(logger.log_record(&record), LogOutcome::Enqueued);
        logger.flush().expect("flush logger");

        let latest = fs::read_to_string(logger.latest_log_path()).expect("read latest");
        let session = fs::read_to_string(&session_path).expect("read session");
        for contents in [&latest, &session] {
            assert!(contents.contains("[ERROR"));
            assert!(contents.contains("[scripting]"));
            assert!(contents.contains("[Python]"));
            assert!(contents.contains("scripts/player.py:42:7"));
            assert!(contents.contains("script failed\\nwithout crashing"));
        }
        assert!(
            session_path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("runtime-"))
        );
        assert!(
            session_path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("log"))
        );
        logger.shutdown().expect("shutdown logger");
        assert!(!logger.stats().worker_running);
    }

    #[test]
    fn rotates_timestamped_log_and_enforces_retention() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut config = test_config(directory.path());
        config.max_file_size_bytes = 220;
        config.retained_files = 2;
        let logger = AsyncLogger::start(config).expect("start logger");

        for index in 0..40 {
            let outcome = logger.log(
                Severity::Info,
                "rotation-test",
                None,
                format!("record {index:03}: {}", "x".repeat(80)),
            );
            assert_eq!(outcome, LogOutcome::Enqueued);
        }
        logger.shutdown().expect("shutdown logger");

        let runtime_files = fs::read_dir(directory.path())
            .expect("read log directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                let path = entry.path();
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("runtime-"))
                    && path
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("log"))
            })
            .count();
        // Active timestamped file plus the configured retained archives.
        assert!(runtime_files <= 3, "found {runtime_files} runtime files");
        assert!(runtime_files >= 2, "rotation did not produce an archive");
    }

    #[test]
    fn filtered_records_do_not_reach_disk() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut config = test_config(directory.path());
        config.filter.minimum_severity = Severity::Warning;
        let logger = AsyncLogger::start(config).expect("start logger");

        assert_eq!(
            logger.log(Severity::Info, "engine", None, "not persisted"),
            LogOutcome::Filtered
        );
        assert_eq!(logger.stats().filtered, 1);
        logger.shutdown().expect("shutdown logger");
        let latest = fs::read_to_string(logger.latest_log_path()).expect("read latest");
        assert!(latest.is_empty());
    }

    #[test]
    fn unavailable_log_directory_is_reported_without_panicking() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let file_instead_of_directory = directory.path().join("occupied");
        fs::write(&file_instead_of_directory, "not a directory").expect("write fixture");

        let result = AsyncLogger::start(test_config(&file_instead_of_directory));

        assert!(matches!(result, Err(LogError::Io(_))));
    }

    #[test]
    fn resilient_logger_keeps_live_stream_when_file_sink_is_unavailable() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let file_instead_of_directory = directory.path().join("occupied");
        fs::write(&file_instead_of_directory, "not a directory").expect("write fixture");
        let logger = AsyncLogger::start_resilient(test_config(&file_instead_of_directory))
            .expect("start resilient logger");
        let live = logger.subscribe(4).unwrap();
        assert_eq!(
            logger.log(Severity::Error, "launcher", None, "visible fallback"),
            LogOutcome::Enqueued
        );
        assert_eq!(
            live.recv_timeout(Duration::from_secs(1)).unwrap().message,
            "visible fallback"
        );
        assert!(!logger.stats().sink_available);
        logger.shutdown().unwrap();
    }

    #[test]
    fn severe_lane_is_reserved_when_normal_records_overflow() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut config = test_config(directory.path());
        config.channel_capacity = 1;
        config.severe_channel_capacity = 4;
        let logger = AsyncLogger::start(config).unwrap();
        for index in 0..5_000 {
            let _ = logger.log(Severity::Debug, "flood", None, format!("normal {index}"));
        }
        let severe = logger.log(Severity::Error, "flood", None, "must use reserved lane");
        assert!(matches!(
            severe,
            LogOutcome::Enqueued | LogOutcome::EnqueuedAfterDroppingOldest
        ));
        logger.shutdown().unwrap();
        let stats = logger.stats();
        assert!(stats.dropped_low_severity > 0);
        assert_eq!(stats.dropped_severe, 0);
        let latest = fs::read_to_string(logger.latest_log_path()).unwrap();
        assert!(latest.contains("must use reserved lane"));
    }
}
