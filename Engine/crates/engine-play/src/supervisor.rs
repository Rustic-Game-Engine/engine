//! Child runtime launch, control, crash observation, and deterministic cleanup.

use crate::changes::RuntimeChangeSet;
use crate::frame_ring::{BgraFrame, FrameRing, FrameRingError, FrameRingLimits};
use crate::local_ipc::{AuthenticatedConnection, LocalEndpoint, connect_authenticated};
use crate::protocol::{
    AuthenticationToken, ConsoleEvent, LiveEntityProperties, ProtocolError, ProtocolMessage,
};
use crate::simulation::{ControlAck, ControlRequest, PlayMode};
use engine_core::ScriptId;
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

const TOKEN_ENVIRONMENT_VARIABLE: &str = "RUSTIC_RUNTIME_IPC_TOKEN";

/// Shell-free child launch contract for `rustic-runtime`.
#[derive(Debug, Clone)]
pub struct RuntimeLaunch {
    pub executable: PathBuf,
    pub snapshot_root: PathBuf,
    pub logs_directory: PathBuf,
    pub endpoint: LocalEndpoint,
    pub mode: PlayMode,
    pub fixed_delta: Duration,
    pub connect_timeout: Duration,
    pub stop_grace_period: Duration,
    pub extra_arguments: Vec<OsString>,
}

impl RuntimeLaunch {
    pub fn new(
        executable: impl Into<PathBuf>,
        snapshot_root: impl Into<PathBuf>,
        logs_directory: impl Into<PathBuf>,
        mode: PlayMode,
        ipc_filesystem_parent: impl AsRef<Path>,
    ) -> Self {
        Self {
            executable: executable.into(),
            snapshot_root: snapshot_root.into(),
            logs_directory: logs_directory.into(),
            endpoint: LocalEndpoint::unique(ipc_filesystem_parent),
            mode,
            fixed_delta: Duration::from_nanos(1_000_000_000 / 60),
            connect_timeout: Duration::from_secs(5),
            stop_grace_period: Duration::from_secs(2),
            extra_arguments: Vec::new(),
        }
    }
}

/// Observed child termination.
#[derive(Debug)]
pub enum SupervisorExit {
    /// Runtime exited within the graceful stop window.
    Graceful(ExitStatus),
    /// Runtime required forced process-tree termination.
    Forced(ExitStatus),
    /// Runtime exited independently (including a crash).
    Exited(ExitStatus),
}

impl SupervisorExit {
    pub fn status(&self) -> ExitStatus {
        match self {
            Self::Graceful(status) | Self::Forced(status) | Self::Exited(status) => *status,
        }
    }
}

/// Editor-owned child and authenticated control channel.
#[derive(Debug)]
pub struct SupervisedRuntime {
    child: Child,
    connection: Option<AuthenticatedConnection>,
    stop_grace_period: Duration,
    next_request_id: u64,
    console_events: VecDeque<ConsoleEvent>,
    frames: FrameRing,
    runtime_changes: Option<RuntimeChangeSet>,
    terminal: bool,
}

impl SupervisedRuntime {
    /// Spawns the runtime in a new process group and waits for authenticated Ready.
    ///
    /// # Errors
    ///
    /// On launch/handshake/readiness failure the child tree is killed and reaped before
    /// the error is returned.
    pub fn spawn(launch: &RuntimeLaunch) -> Result<Self, SupervisorError> {
        let token = AuthenticationToken::generate();
        let mut command = Command::new(&launch.executable);
        command
            .arg("--ipc-server")
            .arg("--ipc-kind")
            .arg(launch.endpoint.kind_argument())
            .arg("--ipc-address")
            .arg(launch.endpoint.address_argument())
            .arg("--snapshot")
            .arg(&launch.snapshot_root)
            .arg("--logs")
            .arg(&launch.logs_directory)
            .arg("--mode")
            .arg(launch.mode.as_str())
            .arg("--fixed-step-nanos")
            .arg(launch.fixed_delta.as_nanos().to_string())
            .args(&launch.extra_arguments)
            .env(TOKEN_ENVIRONMENT_VARIABLE, token.to_secret_hex())
            .stdin(Stdio::null())
            // Runtime diagnostics are transported over the authenticated IPC
            // channel. The editor is a GUI process, so inheriting its standard
            // handles can pass invalid handles to a console child on Windows.
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_group(&mut command);
        let mut child = command.spawn().map_err(|source| SupervisorError::Spawn {
            executable: launch.executable.clone(),
            source,
        })?;
        let connection =
            match connect_authenticated(&launch.endpoint, token, launch.connect_timeout) {
                Ok(connection) => connection,
                Err(error) => {
                    let _ = terminate_tree_and_reap(&mut child);
                    return Err(SupervisorError::Protocol(error));
                }
            };
        let mut runtime = Self {
            child,
            connection: Some(connection),
            stop_grace_period: launch.stop_grace_period,
            next_request_id: 2,
            console_events: VecDeque::new(),
            frames: FrameRing::new(3, FrameRingLimits::bgra(640, 360)?)?,
            runtime_changes: None,
            terminal: false,
        };
        runtime.wait_until_ready(launch.connect_timeout)?;
        Ok(runtime)
    }

    pub fn process_id(&self) -> u32 {
        self.child.id()
    }

    /// Sends the keys currently held by the focused play viewport.
    pub fn set_input_keys(&mut self, keys: Vec<String>) -> Result<(), SupervisorError> {
        if self.terminal {
            return Err(SupervisorError::AlreadyTerminated);
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        self.connection
            .as_mut()
            .ok_or(SupervisorError::AlreadyTerminated)?
            .send(request_id, &ProtocolMessage::InputKeys(keys))?;
        Ok(())
    }

    /// Sends a pause/resume/step/query request and waits for its barrier acknowledgement.
    ///
    /// # Errors
    ///
    /// Returns protocol, timeout, runtime diagnostic, or premature-exit failures.
    pub fn control(
        &mut self,
        request: ControlRequest,
        timeout: Duration,
    ) -> Result<ControlAck, SupervisorError> {
        if self.terminal {
            return Err(SupervisorError::AlreadyTerminated);
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        let connection = self
            .connection
            .as_mut()
            .ok_or(SupervisorError::AlreadyTerminated)?;
        connection.set_receive_timeout(Some(timeout))?;
        connection.send(request_id, &ProtocolMessage::Control(request))?;
        loop {
            let received = connection.receive()?;
            match received.message {
                ProtocolMessage::ControlAck(ack) if received.request_id == request_id => {
                    return Ok(ack);
                }
                ProtocolMessage::Console(event) => self.console_events.push_back(*event),
                ProtocolMessage::Frame(frame) => publish_frame(&self.frames, &frame)?,
                ProtocolMessage::RuntimeChanges(changes) => self.runtime_changes = Some(changes),
                ProtocolMessage::Error { code, message } => {
                    return Err(SupervisorError::Runtime { code, message });
                }
                _ => {}
            }
        }
    }

    /// Reads the selected entity's current runtime properties.
    pub fn query_entity(
        &mut self,
        id: String,
        timeout: Duration,
    ) -> Result<Option<LiveEntityProperties>, SupervisorError> {
        if self.terminal {
            return Err(SupervisorError::AlreadyTerminated);
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        let connection = self
            .connection
            .as_mut()
            .ok_or(SupervisorError::AlreadyTerminated)?;
        connection.set_receive_timeout(Some(timeout))?;
        connection.send(request_id, &ProtocolMessage::QueryEntity(id))?;
        loop {
            let received = connection.receive()?;
            match received.message {
                ProtocolMessage::LiveEntity(entity) if received.request_id == request_id => {
                    return Ok(entity);
                }
                ProtocolMessage::Console(event) => self.console_events.push_back(*event),
                ProtocolMessage::Frame(frame) => publish_frame(&self.frames, &frame)?,
                ProtocolMessage::RuntimeChanges(changes) => self.runtime_changes = Some(changes),
                ProtocolMessage::Error { code, message } => {
                    return Err(SupervisorError::Runtime { code, message });
                }
                _ => {}
            }
        }
    }

    /// Cancels a previously issued request identifier.
    ///
    /// # Errors
    ///
    /// Returns protocol, timeout, runtime diagnostic, or premature-exit failures.
    pub fn cancel_request(
        &mut self,
        cancelled_request_id: u64,
        timeout: Duration,
    ) -> Result<(), SupervisorError> {
        if self.terminal {
            return Err(SupervisorError::AlreadyTerminated);
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        let connection = self
            .connection
            .as_mut()
            .ok_or(SupervisorError::AlreadyTerminated)?;
        connection.set_receive_timeout(Some(timeout))?;
        connection.send(
            request_id,
            &ProtocolMessage::Cancel {
                request_id: cancelled_request_id,
            },
        )?;
        loop {
            let received = connection.receive()?;
            match received.message {
                ProtocolMessage::Cancelled {
                    request_id: cancelled,
                } if received.request_id == request_id && cancelled == cancelled_request_id => {
                    return Ok(());
                }
                ProtocolMessage::Console(event) => self.console_events.push_back(*event),
                ProtocolMessage::Error { code, message } => {
                    return Err(SupervisorError::Runtime { code, message });
                }
                _ => {}
            }
        }
    }

    /// Sends validated source to the authenticated runtime and waits for a safe-boundary result.
    ///
    /// # Errors
    /// Returns protocol, timeout, runtime rejection, frame, or terminated-process failures.
    pub fn reload_script(
        &mut self,
        script_id: ScriptId,
        source: Vec<u8>,
        timeout: Duration,
    ) -> Result<String, SupervisorError> {
        if self.terminal {
            return Err(SupervisorError::AlreadyTerminated);
        }
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        let source_sha256 = format!("{:x}", Sha256::digest(&source));
        let connection = self
            .connection
            .as_mut()
            .ok_or(SupervisorError::AlreadyTerminated)?;
        connection.set_receive_timeout(Some(timeout))?;
        connection.send(
            request_id,
            &ProtocolMessage::ReloadScript {
                format_version: 1,
                script_id,
                api_major: 1,
                source_sha256,
                source,
            },
        )?;
        loop {
            let received = connection.receive()?;
            match received.message {
                ProtocolMessage::ReloadResult {
                    accepted: true,
                    message,
                } if received.request_id == request_id => return Ok(message),
                ProtocolMessage::ReloadResult {
                    accepted: false,
                    message,
                } if received.request_id == request_id => {
                    return Err(SupervisorError::Runtime {
                        code: "reload_rejected".into(),
                        message,
                    });
                }
                ProtocolMessage::Console(event) => self.console_events.push_back(*event),
                ProtocolMessage::Frame(frame) => publish_frame(&self.frames, &frame)?,
                ProtocolMessage::Error { code, message } => {
                    return Err(SupervisorError::Runtime { code, message });
                }
                _ => {}
            }
        }
    }

    /// Takes all live structured console records received while servicing controls.
    pub fn drain_console_events(&mut self) -> impl Iterator<Item = ConsoleEvent> + '_ {
        self.console_events.drain(..)
    }

    /// Takes the reviewed runtime-change proposal, if the runtime supplied one.
    pub fn take_runtime_changes(&mut self) -> Option<RuntimeChangeSet> {
        self.runtime_changes.take()
    }

    /// Consumes the newest complete runtime frame and drops older queued frames.
    ///
    /// # Errors
    ///
    /// Returns an error if the bounded frame ring synchronization was poisoned.
    pub fn take_latest_frame(&self) -> Result<Option<BgraFrame>, SupervisorError> {
        self.frames.consume_latest().map_err(Into::into)
    }

    /// Returns an independently observed exit without blocking.
    ///
    /// # Errors
    ///
    /// Returns child-process observation errors.
    pub fn try_wait(&mut self) -> Result<Option<SupervisorExit>, SupervisorError> {
        let status = self.child.try_wait()?;
        if let Some(status) = status {
            self.terminal = true;
            self.connection = None;
            return Ok(Some(SupervisorExit::Exited(status)));
        }
        Ok(None)
    }

    /// Requests Stop, then forcibly terminates and reaps the process group after grace.
    ///
    /// # Errors
    ///
    /// Returns process observation/termination errors. A broken control connection still
    /// proceeds to forced cleanup.
    pub fn stop(&mut self) -> Result<SupervisorExit, SupervisorError> {
        if self.terminal {
            return Err(SupervisorError::AlreadyTerminated);
        }
        let control_result = self.control(ControlRequest::Stop, self.stop_grace_period);
        if control_result.is_ok()
            && let Some(status) = wait_for_exit(&mut self.child, self.stop_grace_period)?
        {
            self.connection = None;
            self.terminal = true;
            return Ok(SupervisorExit::Graceful(status));
        }
        self.connection = None;
        let status = terminate_tree_and_reap(&mut self.child)?;
        self.terminal = true;
        Ok(SupervisorExit::Forced(status))
    }

    fn wait_until_ready(&mut self, timeout: Duration) -> Result<(), SupervisorError> {
        let connection = self
            .connection
            .as_mut()
            .ok_or(SupervisorError::AlreadyTerminated)?;
        connection.set_receive_timeout(Some(timeout))?;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match connection.receive()?.message {
                ProtocolMessage::Ready { .. } => return Ok(()),
                ProtocolMessage::Console(event) => self.console_events.push_back(*event),
                ProtocolMessage::Frame(frame) => publish_frame(&self.frames, &frame)?,
                ProtocolMessage::Error { code, message } => {
                    return Err(SupervisorError::Runtime { code, message });
                }
                _ => {}
            }
        }
        Err(SupervisorError::ReadyTimedOut)
    }
}

fn publish_frame(frames: &FrameRing, frame: &BgraFrame) -> Result<(), SupervisorError> {
    frames.publish(frame.width, frame.height, frame.stride_bytes, &frame.pixels)?;
    Ok(())
}

impl Drop for SupervisedRuntime {
    fn drop(&mut self) {
        if self.terminal {
            return;
        }
        // Drop never leaves a known child behind. Avoid a second long protocol grace
        // during unwinding; the editor's explicit shutdown path calls `stop` first.
        self.connection = None;
        let _ = terminate_tree_and_reap(&mut self.child);
        self.terminal = true;
    }
}

fn wait_for_exit(
    child: &mut Child,
    timeout: Duration,
) -> Result<Option<ExitStatus>, std::io::Error> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= deadline {
            return Ok(None);
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(windows)]
fn configure_process_group(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(any(windows, unix)))]
fn configure_process_group(_command: &mut Command) {}

fn terminate_tree_and_reap(child: &mut Child) -> Result<ExitStatus, std::io::Error> {
    if let Some(status) = child.try_wait()? {
        return Ok(status);
    }
    terminate_process_group(child.id());
    let _ = child.kill();
    child.wait()
}

#[cfg(windows)]
fn terminate_process_group(process_id: u32) {
    let _ = Command::new("taskkill")
        .args(["/PID", &process_id.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(unix)]
fn terminate_process_group(process_id: u32) {
    let group = format!("-{process_id}");
    let _ = Command::new("kill")
        .args(["-TERM", "--", &group])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    thread::sleep(Duration::from_millis(50));
    let _ = Command::new("kill")
        .args(["-KILL", "--", &group])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(not(any(windows, unix)))]
fn terminate_process_group(_process_id: u32) {}

/// Child launch, protocol, supervision, and cleanup failures.
#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("could not spawn runtime {executable}: {source}")]
    Spawn {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("runtime process operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    #[error(transparent)]
    Frame(#[from] FrameRingError),
    #[error("runtime did not report ready before the deadline")]
    ReadyTimedOut,
    #[error("runtime reported {code}: {message}")]
    Runtime { code: String, message: String },
    #[error("runtime has already terminated")]
    AlreadyTerminated,
}
