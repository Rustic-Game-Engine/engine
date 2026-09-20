//! Authenticated local socket transport backed by `interprocess`.

use crate::protocol::{
    AuthenticationToken, IpcConnection, PROTOCOL_VERSION, ProcessDescriptor, ProcessRole,
    ProtocolError, ProtocolMessage, ProtocolVersion, ReceivedMessage,
};
use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Name, prelude::*,
};
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Cross-platform address for a user-local socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalEndpoint {
    /// Named pipe on Windows or abstract Unix socket where supported.
    Namespaced(String),
    /// Filesystem Unix-domain socket fallback.
    Filesystem(PathBuf),
}

impl LocalEndpoint {
    /// Creates a collision-resistant endpoint, preferring non-filesystem namespaces.
    pub fn unique(filesystem_parent: impl AsRef<Path>) -> Self {
        let token = Uuid::new_v4();
        if GenericNamespaced::is_supported() {
            Self::Namespaced(format!("rustic-runtime-{token}"))
        } else {
            Self::Filesystem(
                filesystem_parent
                    .as_ref()
                    .join(format!("runtime-{token}.sock")),
            )
        }
    }

    /// Stable argument identifying the endpoint representation.
    pub const fn kind_argument(&self) -> &'static str {
        match self {
            Self::Namespaced(_) => "namespaced",
            Self::Filesystem(_) => "filesystem",
        }
    }

    /// Native endpoint value suitable for a shell-free process argument.
    pub fn address_argument(&self) -> OsString {
        match self {
            Self::Namespaced(name) => OsString::from(name),
            Self::Filesystem(path) => path.as_os_str().to_owned(),
        }
    }

    /// Reconstructs an endpoint from shell-free native arguments.
    ///
    /// # Errors
    ///
    /// Returns invalid-input when `kind` is unknown or a namespaced address is not UTF-8.
    pub fn from_arguments(kind: &str, address: &OsStr) -> io::Result<Self> {
        match kind {
            "namespaced" => address.to_str().map_or_else(
                || {
                    Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "namespaced IPC address is not UTF-8",
                    ))
                },
                |value| Ok(Self::Namespaced(value.to_owned())),
            ),
            "filesystem" => Ok(Self::Filesystem(PathBuf::from(address))),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown IPC endpoint kind",
            )),
        }
    }

    fn name(&self) -> io::Result<Name<'_>> {
        match self {
            Self::Namespaced(name) => name.as_str().to_ns_name::<GenericNamespaced>(),
            Self::Filesystem(path) => path.as_path().to_fs_name::<GenericFilePath>(),
        }
    }
}

/// Successfully authenticated connection and peer identity.
#[derive(Debug)]
pub struct AuthenticatedConnection {
    connection: IpcConnection<LocalSocketStream>,
    peer: ProcessDescriptor,
}

impl AuthenticatedConnection {
    pub fn peer(&self) -> &ProcessDescriptor {
        &self.peer
    }

    pub const fn negotiated_version(&self) -> ProtocolVersion {
        self.connection.negotiated_version()
    }

    /// Sends one protocol message.
    ///
    /// # Errors
    ///
    /// Returns framing or local-socket I/O errors.
    pub fn send(
        &mut self,
        request_id: u64,
        message: &ProtocolMessage,
    ) -> Result<(), ProtocolError> {
        self.connection.send(request_id, message)
    }

    /// Receives one validated protocol message.
    ///
    /// # Errors
    ///
    /// Returns framing, compatibility, deadline, decode, or local-socket I/O errors.
    pub fn receive(&mut self) -> Result<ReceivedMessage, ProtocolError> {
        self.connection.receive()
    }

    /// Configures bounded receive waits for supervision and scheduling loops.
    ///
    /// # Errors
    ///
    /// Returns the platform local-socket error.
    pub fn set_receive_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        tolerate_unsupported_timeout(self.connection_stream().set_recv_timeout(timeout))
    }

    fn connection_stream(&self) -> &LocalSocketStream {
        // IpcConnection deliberately owns its transport. This private layout-preserving
        // helper is implemented through a borrow accessor below.
        self.connection.stream_ref()
    }
}

/// One-use authenticated local listener.
#[derive(Debug)]
pub struct LocalIpcListener {
    listener: LocalSocketListener,
    token: AuthenticationToken,
    consumed: bool,
}

impl LocalIpcListener {
    /// Binds a local endpoint. Filesystem sockets are restricted to the current user on Unix.
    ///
    /// # Errors
    ///
    /// Returns directory creation, name conversion, or listener creation errors.
    pub fn bind(endpoint: &LocalEndpoint, token: AuthenticationToken) -> io::Result<Self> {
        if let LocalEndpoint::Filesystem(path) = endpoint
            && let Some(parent) = path.parent()
        {
            std::fs::create_dir_all(parent)?;
        }
        let options = ListenerOptions::new().name(endpoint.name()?);
        #[cfg(unix)]
        let options = {
            use interprocess::os::unix::local_socket::ListenerOptionsExt;
            options.mode(0o600)
        };
        let listener = options.create_sync()?;
        Ok(Self {
            listener,
            token,
            consumed: false,
        })
    }

    /// Accepts and authenticates the single editor connection.
    ///
    /// # Errors
    ///
    /// Rejects replay, wrong roles/tokens, incompatible minor requirements, and all
    /// protocol or socket failures.
    pub fn accept_authenticated(
        &mut self,
        runtime_process: ProcessDescriptor,
    ) -> Result<AuthenticatedConnection, ProtocolError> {
        if self.consumed {
            return Err(ProtocolError::AuthenticationReplay);
        }
        let stream = self.listener.accept()?;
        let mut connection = IpcConnection::new(stream, ProcessRole::Runtime);
        let hello = connection.receive()?;
        if hello.sender != ProcessRole::Editor {
            return Err(ProtocolError::AuthenticationFailed);
        }
        let ProtocolMessage::ClientHello {
            token,
            minimum_minor,
        } = hello.message
        else {
            return Err(ProtocolError::UnexpectedMessage("client hello"));
        };
        if !token.securely_matches(&self.token) {
            return Err(ProtocolError::AuthenticationFailed);
        }
        if minimum_minor > PROTOCOL_VERSION.minor {
            return Err(ProtocolError::IncompatibleMinor {
                minimum: minimum_minor,
                supported: PROTOCOL_VERSION.minor,
            });
        }
        self.consumed = true;
        let negotiated = ProtocolVersion {
            major: PROTOCOL_VERSION.major,
            minor: PROTOCOL_VERSION.minor,
        };
        connection.set_negotiated_version(negotiated);
        connection.send(
            hello.request_id,
            &ProtocolMessage::ServerHello {
                negotiated,
                process: runtime_process.clone(),
            },
        )?;
        Ok(AuthenticatedConnection {
            connection,
            peer: runtime_process,
        })
    }
}

/// Connects to a runtime and completes the authenticated handshake before the deadline.
///
/// # Errors
///
/// Returns connection, authentication, compatibility, or handshake framing errors.
pub fn connect_authenticated(
    endpoint: &LocalEndpoint,
    token: AuthenticationToken,
    timeout: Duration,
) -> Result<AuthenticatedConnection, ProtocolError> {
    let deadline = Instant::now() + timeout;
    let stream = loop {
        match LocalSocketStream::connect(endpoint.name()?) {
            Ok(stream) => break stream,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound
                        | io::ErrorKind::ConnectionRefused
                        | io::ErrorKind::AddrNotAvailable
                ) && Instant::now() < deadline =>
            {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(ProtocolError::Io(error)),
        }
    };
    let remaining = deadline.saturating_duration_since(Instant::now());
    tolerate_unsupported_timeout(
        stream.set_recv_timeout(Some(remaining.max(Duration::from_millis(1)))),
    )?;
    tolerate_unsupported_timeout(
        stream.set_send_timeout(Some(remaining.max(Duration::from_millis(1)))),
    )?;
    let mut connection = IpcConnection::new(stream, ProcessRole::Editor);
    connection.send(
        1,
        &ProtocolMessage::ClientHello {
            token,
            minimum_minor: 0,
        },
    )?;
    let response = connection.receive()?;
    if response.sender != ProcessRole::Runtime {
        return Err(ProtocolError::AuthenticationFailed);
    }
    let ProtocolMessage::ServerHello {
        negotiated,
        process,
    } = response.message
    else {
        return Err(ProtocolError::UnexpectedMessage("server hello"));
    };
    if negotiated.major != PROTOCOL_VERSION.major || negotiated.minor > PROTOCOL_VERSION.minor {
        return Err(ProtocolError::IncompatibleMajor {
            found: negotiated.major,
            expected: PROTOCOL_VERSION.major,
        });
    }
    connection.set_negotiated_version(negotiated);
    Ok(AuthenticatedConnection {
        connection,
        peer: process,
    })
}

fn tolerate_unsupported_timeout(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::Unsupported => Ok(()),
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::ControlRequest;
    use std::sync::mpsc;

    fn runtime_process() -> ProcessDescriptor {
        ProcessDescriptor {
            role: ProcessRole::Runtime,
            process_id: std::process::id(),
            name: "test-runtime".to_owned(),
        }
    }

    #[test]
    fn local_socket_handshake_authenticates_and_transports_a_request() {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = LocalEndpoint::unique(directory.path());
        let token = AuthenticationToken::generate();
        let server_endpoint = endpoint.clone();
        let server_token = token.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut listener = LocalIpcListener::bind(&server_endpoint, server_token).unwrap();
            ready_tx.send(()).unwrap();
            let mut connection = listener.accept_authenticated(runtime_process()).unwrap();
            let request = connection.receive().unwrap();
            assert_eq!(
                request.message,
                ProtocolMessage::Control(ControlRequest::Pause)
            );
            connection
                .send(
                    request.request_id,
                    &ProtocolMessage::Error {
                        code: "test".to_owned(),
                        message: "received".to_owned(),
                    },
                )
                .unwrap();
            assert!(matches!(
                listener.accept_authenticated(runtime_process()),
                Err(ProtocolError::AuthenticationReplay)
            ));
        });
        ready_rx.recv().unwrap();
        let mut client = connect_authenticated(&endpoint, token, Duration::from_secs(2)).unwrap();
        assert_eq!(client.peer().role, ProcessRole::Runtime);
        client
            .send(9, &ProtocolMessage::Control(ControlRequest::Pause))
            .unwrap();
        let response = client.receive().unwrap();
        assert_eq!(response.request_id, 9);
        server.join().unwrap();
    }

    #[test]
    fn wrong_token_is_rejected_without_consuming_the_expected_secret() {
        let directory = tempfile::tempdir().unwrap();
        let endpoint = LocalEndpoint::unique(directory.path());
        let expected = AuthenticationToken::generate();
        let wrong = AuthenticationToken::generate();
        let server_endpoint = endpoint.clone();
        let (ready_tx, ready_rx) = mpsc::channel();
        let server = thread::spawn(move || {
            let mut listener = LocalIpcListener::bind(&server_endpoint, expected).unwrap();
            ready_tx.send(()).unwrap();
            assert!(matches!(
                listener.accept_authenticated(runtime_process()),
                Err(ProtocolError::AuthenticationFailed)
            ));
        });
        ready_rx.recv().unwrap();
        let result = connect_authenticated(&endpoint, wrong, Duration::from_secs(2));
        assert!(result.is_err());
        server.join().unwrap();
    }
}
