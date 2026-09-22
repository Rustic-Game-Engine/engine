//! Runtime-side authenticated control server.

use crate::frame_ring::BgraFrame;
use crate::local_ipc::{AuthenticatedConnection, LocalEndpoint, LocalIpcListener};
use crate::protocol::{
    AuthenticationToken, ConsoleEvent, ConsoleRecord, ProcessDescriptor, ProcessRole,
    ProtocolError, ProtocolMessage,
};
use crate::script_runtime::RuntimeScripts;
use crate::simulation::{
    ControlAck, ControlRequest, PlayMode, RuntimeSimulation, RuntimeState, SimulationError,
};
use crate::snapshot::{PlaySnapshot, SnapshotError};
use engine_core::logging::{AsyncLogger, LogConfig, LogMetadata, LogRecord, LogStream, Severity};
use sha2::{Digest, Sha256};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Complete runtime-server startup contract.
#[derive(Debug, Clone)]
pub struct RuntimeServerConfig {
    pub endpoint: LocalEndpoint,
    pub authentication_token: AuthenticationToken,
    pub snapshot_root: PathBuf,
    pub logs_directory: PathBuf,
    pub mode: PlayMode,
    pub fixed_delta: Duration,
    /// Deterministic failure injection used by supervision/recovery tests.
    pub crash_after_ready: bool,
    /// Deterministic unresponsive-stop fixture used to prove forced cleanup.
    pub ignore_stop: bool,
}

/// Runs one authenticated editor session until Stop/disconnect.
///
/// # Errors
///
/// Returns snapshot validation, listener/authentication/protocol, simulation, or
/// synchronization failures. Persistent log startup failure is non-fatal and falls
/// back to standard error.
///
/// # Panics
///
/// Panics only when the explicit `crash_after_ready` failure-injection flag is set.
#[allow(
    clippy::too_many_lines,
    reason = "the session loop keeps control ordering and shutdown cleanup auditable"
)]
pub fn run_runtime_server(config: RuntimeServerConfig) -> Result<(), RuntimeServerError> {
    let mode = config.mode;
    run_runtime_server_with_renderer(config, move |_, tick| Ok(runtime_frame(mode, tick)))
}

/// Runs the protocol with an application-owned renderer of the live simulation world.
pub fn run_runtime_server_with_renderer(
    config: RuntimeServerConfig,
    mut render: impl FnMut(&engine_world::SceneWorld, u64) -> Result<BgraFrame, String>,
) -> Result<(), RuntimeServerError> {
    let snapshot = PlaySnapshot::open(&config.snapshot_root)?;
    let source_scene = snapshot.scene_bytes()?;
    let scripts = Arc::new(Mutex::new(
        RuntimeScripts::load(snapshot.root(), &source_scene)
            .map_err(RuntimeServerError::Scripts)?,
    ));
    if snapshot.manifest().mode != config.mode {
        return Err(RuntimeServerError::SnapshotModeMismatch {
            snapshot: snapshot.manifest().mode,
            requested: config.mode,
        });
    }
    let process = ProcessDescriptor {
        role: ProcessRole::Runtime,
        process_id: std::process::id(),
        name: "rustic-runtime".to_owned(),
    };
    let logger = start_logger(&config.logs_directory, LogStream::Runtime);
    let mut listener = LocalIpcListener::bind(&config.endpoint, config.authentication_token)?;
    let mut connection = listener.accept_authenticated(process.clone())?;
    let simulation = Arc::new(Mutex::new(RuntimeSimulation::new(
        config.mode,
        config.fixed_delta,
    )?));
    let stopping = Arc::new(AtomicBool::new(false));
    let simulation_worker = start_simulation_worker(
        Arc::clone(&simulation),
        Arc::clone(&scripts),
        Arc::clone(&stopping),
    )?;

    let result = (|| {
        connection.send(
            0,
            &ProtocolMessage::Frame(
                scripts
                    .lock()
                    .map_err(|_| RuntimeServerError::SimulationPoisoned)?
                    .render_world(|world| render(world, 0))
                    .map_err(RuntimeServerError::Scripts)?
                    .map_err(RuntimeServerError::Scripts)?,
            ),
        )?;
        connection.send(
            0,
            &ProtocolMessage::Ready {
                mode: config.mode,
                fixed_tick: 0,
            },
        )?;
        assert!(
            !config.crash_after_ready,
            "injected runtime crash after readiness"
        );
        emit_console(
            &mut connection,
            logger.as_ref(),
            &process,
            Severity::Info,
            "runtime",
            format!("{} runtime ready", config.mode.as_str()),
        )?;
        emit_script_logs(&mut connection, logger.as_ref(), &process, &scripts)?;

        loop {
            let received = match connection.receive() {
                Ok(message) => message,
                Err(ProtocolError::Io(error))
                    if matches!(
                        error.kind(),
                        io::ErrorKind::UnexpectedEof
                            | io::ErrorKind::BrokenPipe
                            | io::ErrorKind::ConnectionReset
                    ) =>
                {
                    break;
                }
                Err(error) => return Err(RuntimeServerError::Protocol(error)),
            };
            if received.sender != ProcessRole::Editor {
                return Err(RuntimeServerError::WrongPeerRole(received.sender));
            }
            let request = match received.message {
                ProtocolMessage::Control(request) => request,
                ProtocolMessage::InputKeys(keys) => {
                    scripts
                        .lock()
                        .map_err(|_| RuntimeServerError::SimulationPoisoned)?
                        .set_input_keys(keys);
                    continue;
                }
                ProtocolMessage::QueryEntity(id) => {
                    let entity = id.parse().ok();
                    let live = match entity {
                        Some(entity) => scripts
                            .lock()
                            .map_err(|_| RuntimeServerError::SimulationPoisoned)?
                            .live_entity(entity)
                            .map_err(RuntimeServerError::Scripts)?,
                        None => None,
                    };
                    connection.send(received.request_id, &ProtocolMessage::LiveEntity(live))?;
                    continue;
                }
                ProtocolMessage::ReloadScript {
                    format_version,
                    script_id,
                    api_major,
                    source_sha256,
                    source,
                } => {
                    let actual = format!("{:x}", Sha256::digest(&source));
                    let result = if format_version != 1 {
                        Err(format!("unsupported reload version {format_version}"))
                    } else if api_major != 1 {
                        Err(format!("incompatible gameplay API major {api_major}"))
                    } else if actual != source_sha256 {
                        Err("reload source hash mismatch".to_owned())
                    } else {
                        scripts
                            .lock()
                            .map_err(|_| RuntimeServerError::SimulationPoisoned)?
                            .reload(script_id, &source)
                            .map(|count| format!("reloaded {count} running instance(s)"))
                    };
                    let (accepted, message) = match result {
                        Ok(message) => (true, message),
                        Err(message) => (
                            false,
                            format!(
                                "reload rejected; last-known-good behavior retained: {message}"
                            ),
                        ),
                    };
                    connection.send(
                        received.request_id,
                        &ProtocolMessage::ReloadResult { accepted, message },
                    )?;
                    continue;
                }
                ProtocolMessage::Cancel { request_id } => {
                    connection.send(
                        received.request_id,
                        &ProtocolMessage::Cancelled { request_id },
                    )?;
                    continue;
                }
                _ => {
                    connection.send(
                        received.request_id,
                        &ProtocolMessage::Error {
                            code: "unexpected_message".to_owned(),
                            message: "runtime accepts control, reload, and cancel requests after authentication"
                                .to_owned(),
                        },
                    )?;
                    continue;
                }
            };
            emit_script_logs(&mut connection, logger.as_ref(), &process, &scripts)?;
            let ack = if request == ControlRequest::Stop && config.ignore_stop {
                let simulation = simulation
                    .lock()
                    .map_err(|_| RuntimeServerError::SimulationPoisoned)?;
                ControlAck {
                    request,
                    state: simulation.state(),
                    fixed_tick: simulation.fixed_tick(),
                }
            } else {
                simulation
                    .lock()
                    .map_err(|_| RuntimeServerError::SimulationPoisoned)?
                    .control(request)?
            };
            if request == ControlRequest::FrameAdvance {
                scripts
                    .lock()
                    .map_err(|_| RuntimeServerError::SimulationPoisoned)?
                    .fixed_update(config.fixed_delta.as_secs_f64());
            }
            if request == ControlRequest::Stop {
                let mut scripts = scripts
                    .lock()
                    .map_err(|_| RuntimeServerError::SimulationPoisoned)?;
                scripts.stop();
                let changes = scripts
                    .runtime_changes(snapshot.source_scene_hash(), &source_scene)
                    .map_err(RuntimeServerError::Scripts)?;
                connection.send(
                    received.request_id,
                    &ProtocolMessage::RuntimeChanges(changes),
                )?;
            } else {
                connection.send(
                    received.request_id,
                    &ProtocolMessage::Frame(
                        scripts
                            .lock()
                            .map_err(|_| RuntimeServerError::SimulationPoisoned)?
                            .render_world(|world| render(world, ack.fixed_tick))
                            .map_err(RuntimeServerError::Scripts)?
                            .map_err(RuntimeServerError::Scripts)?,
                    ),
                )?;
            }
            if request != ControlRequest::QueryState {
                emit_console(
                    &mut connection,
                    logger.as_ref(),
                    &process,
                    Severity::Debug,
                    "runtime.control",
                    format!(
                        "control {request:?}: state={:?}, fixed_tick={}",
                        ack.state, ack.fixed_tick
                    ),
                )?;
            }
            connection.send(received.request_id, &ProtocolMessage::ControlAck(ack))?;
            if ack.state == RuntimeState::Stopped {
                break;
            }
        }
        Ok(())
    })();

    stopping.store(true, Ordering::Release);
    if simulation_worker.join().is_err() && result.is_ok() {
        return Err(RuntimeServerError::SimulationWorkerPanicked);
    }
    if let Some(logger) = &logger {
        let _ = logger.shutdown();
    }
    result
}

fn runtime_frame(mode: PlayMode, fixed_tick: u64) -> BgraFrame {
    const WIDTH: u32 = 64;
    const HEIGHT: u32 = 64;
    const STRIDE: u32 = WIDTH * 4;
    let mode_color = match mode {
        PlayMode::Play => [48_u8, 140, 70],
        PlayMode::NewWindow => [150_u8, 92, 42],
        PlayMode::Standalone => [108_u8, 62, 156],
    };
    let pulse = u8::try_from(fixed_tick % 96).unwrap_or_default();
    let mut pixels = vec![0_u8; usize::try_from(STRIDE * HEIGHT).unwrap_or_default()];
    for (index, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let checker = ((index % usize::try_from(WIDTH).unwrap_or(1)) / 8
            + (index / usize::try_from(WIDTH).unwrap_or(1)) / 8)
            .is_multiple_of(2);
        let shade = if checker { pulse } else { 0 };
        pixel.copy_from_slice(&[
            mode_color[2].saturating_add(shade),
            mode_color[1].saturating_add(shade),
            mode_color[0].saturating_add(shade),
            255,
        ]);
    }
    BgraFrame {
        sequence: fixed_tick,
        width: WIDTH,
        height: HEIGHT,
        stride_bytes: STRIDE,
        pixels,
    }
}

/// Writes a best-effort crash-stream record without allowing sink failure to panic.
pub fn record_runtime_crash(logs_directory: &std::path::Path, message: impl Into<String>) {
    let message = message.into();
    if let Some(logger) = start_logger(logs_directory, LogStream::Crash) {
        let _ = logger.log(Severity::Fatal, "runtime.crash", None, message);
        let _ = logger.shutdown();
    } else {
        eprintln!("rustic-runtime crash: {message}");
    }
}

fn start_logger(directory: &std::path::Path, stream: LogStream) -> Option<AsyncLogger> {
    match AsyncLogger::start(LogConfig {
        directory: directory.to_path_buf(),
        stream,
        ..LogConfig::default()
    }) {
        Ok(logger) => Some(logger),
        Err(error) => {
            eprintln!("runtime log sink unavailable: {error}");
            None
        }
    }
}

fn emit_console(
    connection: &mut AuthenticatedConnection,
    logger: Option<&AsyncLogger>,
    process: &ProcessDescriptor,
    severity: Severity,
    subsystem: &str,
    message: String,
) -> Result<(), ProtocolError> {
    let record = LogRecord::new(severity, message, LogMetadata::new(subsystem));
    println!("[{}] [{}] {}", severity.as_str(), subsystem, record.message);
    if let Some(logger) = logger {
        let _ = logger.log_record(&record);
    }
    connection.send(
        0,
        &ProtocolMessage::Console(Box::new(ConsoleEvent {
            process: process.clone(),
            record: ConsoleRecord::from_log_record(&record),
            stack_frames: Vec::new(),
        })),
    )
}

fn emit_script_logs(
    connection: &mut AuthenticatedConnection,
    logger: Option<&AsyncLogger>,
    process: &ProcessDescriptor,
    scripts: &Arc<Mutex<RuntimeScripts>>,
) -> Result<(), RuntimeServerError> {
    let scripts = scripts
        .lock()
        .map_err(|_| RuntimeServerError::SimulationPoisoned)?;
    for (level, message) in scripts.drain_logs() {
        let severity = match level.as_str() {
            "error" => Severity::Error,
            "warning" | "warn" => Severity::Warning,
            "debug" => Severity::Debug,
            _ => Severity::Info,
        };
        emit_console(connection, logger, process, severity, "script", message)?;
    }
    Ok(())
}

fn start_simulation_worker(
    simulation: Arc<Mutex<RuntimeSimulation>>,
    scripts: Arc<Mutex<RuntimeScripts>>,
    stopping: Arc<AtomicBool>,
) -> Result<thread::JoinHandle<()>, io::Error> {
    thread::Builder::new()
        .name("rustic-fixed-simulation".to_owned())
        .spawn(move || {
            let mut previous = Instant::now();
            while !stopping.load(Ordering::Acquire) {
                let now = Instant::now();
                let elapsed = now.saturating_duration_since(previous);
                previous = now;
                let Ok(mut simulation) = simulation.lock() else {
                    return;
                };
                let ticks = simulation.advance_elapsed(elapsed);
                let running = simulation.state() == RuntimeState::Running;
                let fixed_delta = simulation.fixed_delta().as_secs_f64();
                drop(simulation);
                if running && let Ok(mut scripts) = scripts.lock() {
                    for _ in 0..ticks {
                        scripts.fixed_update(fixed_delta);
                    }
                    scripts.frame_update(elapsed.as_secs_f64());
                }
                thread::sleep(Duration::from_millis(1));
            }
        })
}

/// Runtime-server startup and session failures.
#[derive(Debug, Error)]
pub enum RuntimeServerError {
    #[error(transparent)]
    Snapshot(#[from] SnapshotError),
    #[error("snapshot mode {snapshot:?} does not match requested mode {requested:?}")]
    SnapshotModeMismatch {
        snapshot: PlayMode,
        requested: PlayMode,
    },
    #[error("runtime local IPC failed: {0}")]
    Io(#[from] io::Error),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Simulation(#[from] SimulationError),
    #[error("runtime received a post-handshake frame from {0:?}")]
    WrongPeerRole(ProcessRole),
    #[error("runtime simulation synchronization was poisoned")]
    SimulationPoisoned,
    #[error("runtime simulation worker panicked")]
    SimulationWorkerPanicked,
    #[error("runtime script startup/execution failed: {0}")]
    Scripts(String),
}
