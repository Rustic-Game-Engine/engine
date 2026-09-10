use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Shell-free description of a child process launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRequest {
    program: PathBuf,
    arguments: Vec<OsString>,
    working_directory: Option<PathBuf>,
    clear_environment: bool,
    environment: BTreeMap<OsString, OsString>,
    terminate_on_drop: bool,
}

impl ProcessRequest {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            arguments: Vec::new(),
            working_directory: None,
            clear_environment: false,
            environment: BTreeMap::new(),
            terminate_on_drop: false,
        }
    }

    #[must_use]
    pub fn argument(mut self, argument: impl Into<OsString>) -> Self {
        self.arguments.push(argument.into());
        self
    }

    #[must_use]
    pub fn working_directory(mut self, directory: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(directory.into());
        self
    }

    /// Clears inherited variables and retains only a small OS bootstrap set.
    #[must_use]
    pub fn sanitized_environment(mut self) -> Self {
        self.clear_environment = true;
        for name in ["PATH", "SystemRoot", "WINDIR", "TEMP", "TMP", "HOME"] {
            if let Some(value) = std::env::var_os(name) {
                self.environment.insert(name.into(), value);
            }
        }
        self
    }

    #[must_use]
    pub fn environment(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.environment.insert(name.into(), value.into());
        self
    }

    /// Requests best-effort termination when the last supervisor is dropped.
    #[must_use]
    pub fn terminate_on_drop(mut self, enabled: bool) -> Self {
        self.terminate_on_drop = enabled;
        self
    }

    pub fn program(&self) -> &Path {
        &self.program
    }

    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    pub fn working_directory_path(&self) -> Option<&Path> {
        self.working_directory.as_deref()
    }
}

/// Process service failure.
#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("could not launch {program}: {source}")]
    Spawn {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("process {process_id} operation failed: {source}")]
    Operation {
        process_id: u32,
        #[source]
        source: std::io::Error,
    },
    #[error("process {0} did not exit before the deadline")]
    TimedOut(u32),
}

/// Minimal detached process identity retained by legacy callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnedProcess {
    id: u32,
}

impl SpawnedProcess {
    pub const fn id(self) -> u32 {
        self.id
    }
}

/// Stable exit result independent of `std::process` layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessExit {
    pub code: Option<i32>,
    pub success: bool,
}

impl From<ExitStatus> for ProcessExit {
    fn from(status: ExitStatus) -> Self {
        Self {
            code: status.code(),
            success: status.success(),
        }
    }
}

/// Owned, thread-safe child-process supervisor.
#[derive(Debug, Clone)]
pub struct SupervisedProcess {
    id: u32,
    child: Arc<Mutex<Option<Child>>>,
    terminate_on_drop: bool,
}

impl SupervisedProcess {
    pub const fn id(&self) -> u32 {
        self.id
    }

    /// Polls the child without blocking.
    ///
    /// # Errors
    ///
    /// Returns an error when the operating system cannot query the child state.
    pub fn try_wait(&self) -> Result<Option<ProcessExit>, ProcessError> {
        let mut child = self.child.lock();
        let Some(child) = child.as_mut() else {
            return Ok(None);
        };
        child
            .try_wait()
            .map(|status| status.map(ProcessExit::from))
            .map_err(|source| ProcessError::Operation {
                process_id: self.id,
                source,
            })
    }

    /// Waits up to `timeout` for the child to exit.
    ///
    /// # Errors
    ///
    /// Returns an error when child status cannot be queried or the timeout expires.
    pub fn wait_timeout(&self, timeout: Duration) -> Result<ProcessExit, ProcessError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(exit) = self.try_wait()? {
                return Ok(exit);
            }
            if Instant::now() >= deadline {
                return Err(ProcessError::TimedOut(self.id));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Terminates the child and waits for it to be reaped.
    ///
    /// # Errors
    ///
    /// Returns an error when the child cannot be killed or reaped.
    pub fn terminate(&self) -> Result<ProcessExit, ProcessError> {
        let mut child = self.child.lock();
        let Some(mut child) = child.take() else {
            return Ok(ProcessExit {
                code: None,
                success: true,
            });
        };
        if child
            .try_wait()
            .map_err(|source| ProcessError::Operation {
                process_id: self.id,
                source,
            })?
            .is_none()
        {
            child.kill().map_err(|source| ProcessError::Operation {
                process_id: self.id,
                source,
            })?;
        }
        child
            .wait()
            .map(ProcessExit::from)
            .map_err(|source| ProcessError::Operation {
                process_id: self.id,
                source,
            })
    }
}

impl Drop for SupervisedProcess {
    fn drop(&mut self) {
        if self.terminate_on_drop && Arc::strong_count(&self.child) == 1 {
            let _ = self.terminate();
        }
    }
}

/// Injectable process boundary used by product services.
pub trait ProcessLauncher {
    /// Starts a detached child process.
    ///
    /// # Errors
    ///
    /// Returns an error when process creation fails.
    fn spawn(&self, request: &ProcessRequest) -> Result<SpawnedProcess, ProcessError>;
    /// Starts a child process whose lifecycle remains owned by the returned supervisor.
    ///
    /// # Errors
    ///
    /// Returns an error when process creation fails.
    fn spawn_supervised(&self, request: &ProcessRequest)
    -> Result<SupervisedProcess, ProcessError>;
}

/// Native implementation backed by [`Command`].
#[derive(Debug, Default, Clone, Copy)]
pub struct NativeProcessLauncher;

impl NativeProcessLauncher {
    fn command(request: &ProcessRequest) -> Command {
        let mut command = Command::new(&request.program);
        command.args(&request.arguments);
        if let Some(directory) = &request.working_directory {
            command.current_dir(directory);
        }
        if request.clear_environment {
            command.env_clear();
        }
        command.envs(&request.environment);
        command
    }
}

impl ProcessLauncher for NativeProcessLauncher {
    fn spawn(&self, request: &ProcessRequest) -> Result<SpawnedProcess, ProcessError> {
        let child = Self::command(request)
            .spawn()
            .map_err(|source| ProcessError::Spawn {
                program: request.program.clone(),
                source,
            })?;
        Ok(SpawnedProcess { id: child.id() })
    }

    fn spawn_supervised(
        &self,
        request: &ProcessRequest,
    ) -> Result<SupervisedProcess, ProcessError> {
        let child = Self::command(request)
            .spawn()
            .map_err(|source| ProcessError::Spawn {
                program: request.program.clone(),
                source,
            })?;
        let id = child.id();
        Ok(SupervisedProcess {
            id,
            child: Arc::new(Mutex::new(Some(child))),
            terminate_on_drop: request.terminate_on_drop,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn process_request_preserves_native_argument_boundaries() {
        let request = ProcessRequest::new("rustic-editor")
            .argument("--project")
            .argument("Projects/My Game; still one argument");
        assert_eq!(request.arguments().len(), 2);
        assert_eq!(request.arguments()[0], OsStr::new("--project"));
        assert_eq!(
            request.arguments()[1],
            OsStr::new("Projects/My Game; still one argument")
        );
    }

    #[test]
    fn sanitized_environment_retains_explicit_values() {
        let request = ProcessRequest::new("program")
            .sanitized_environment()
            .environment("RUSTIC_AUTH_TOKEN", "secret");
        assert!(request.clear_environment);
        assert_eq!(
            request.environment.get(OsStr::new("RUSTIC_AUTH_TOKEN")),
            Some(&OsString::from("secret"))
        );
    }
}
