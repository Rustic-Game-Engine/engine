//! One-shot isolated decoder protocol and supervisor.

use crate::{AssetError, DerivedArtifact, ImportRequest, ImporterRegistry};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

const WORKER_MAGIC: [u8; 4] = *b"RAW1";
/// Worker protocol version.
pub const ASSET_WORKER_PROTOCOL_VERSION: u16 = 1;
/// Maximum framed request/response bytes.
pub const MAX_WORKER_FRAME_BYTES: usize = 64 * 1024 * 1024;

/// Authenticated one-shot worker request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetWorkerRequest {
    /// Protocol version.
    pub protocol_version: u16,
    /// One-use supervisor token.
    pub token: Uuid,
    /// Source/dependency bytes.
    pub import: ImportRequest,
}

/// Worker response containing either an engine-owned artifact or diagnostic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetWorkerResponse {
    /// Protocol version.
    pub protocol_version: u16,
    /// Echoed one-use token.
    pub token: Uuid,
    /// Successful output.
    pub artifact: Option<DerivedArtifact>,
    /// Failure diagnostic.
    pub error: Option<String>,
}

/// Runs one worker request over stdin/stdout. Used only by `rustic-asset-worker`.
///
/// # Errors
///
/// Returns an error for malformed or unauthenticated frames, import failure, or output failure.
pub fn run_worker_stdio(expected_token: Uuid) -> Result<(), AssetError> {
    let request: AssetWorkerRequest = read_frame(std::io::stdin().lock())?;
    if request.protocol_version != ASSET_WORKER_PROTOCOL_VERSION || request.token != expected_token
    {
        return Err(AssetError::Worker(
            "worker protocol version or authentication token mismatch".to_owned(),
        ));
    }
    let result = ImporterRegistry::import(&request.import);
    let response = match result {
        Ok(artifact) => AssetWorkerResponse {
            protocol_version: ASSET_WORKER_PROTOCOL_VERSION,
            token: request.token,
            artifact: Some(artifact),
            error: None,
        },
        Err(error) => AssetWorkerResponse {
            protocol_version: ASSET_WORKER_PROTOCOL_VERSION,
            token: request.token,
            artifact: None,
            error: Some(error.to_string()),
        },
    };
    write_frame(std::io::stdout().lock(), &response)
}

/// Engine-owned process supervisor for risky decoders.
#[derive(Debug, Clone)]
pub struct IsolatedImporter {
    /// Worker executable path.
    pub program: PathBuf,
    /// Per-project working directory.
    pub working_directory: PathBuf,
    /// Hard process deadline.
    pub timeout: Duration,
}

impl IsolatedImporter {
    /// Executes one import in a sanitized one-shot child and always reaps it.
    ///
    /// # Errors
    ///
    /// Returns an error when the child cannot be started or supervised, times out, crashes,
    /// violates the framed protocol, or reports an import failure.
    pub fn import(&self, import: ImportRequest) -> Result<DerivedArtifact, AssetError> {
        let token = Uuid::new_v4();
        let request = AssetWorkerRequest {
            protocol_version: ASSET_WORKER_PROTOCOL_VERSION,
            token,
            import,
        };
        let mut command = Command::new(&self.program);
        command
            .arg("--worker-stdio")
            .arg("--token")
            .arg(token.to_string())
            .current_dir(&self.working_directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear();
        preserve_minimal_environment(&mut command);
        let mut child = command
            .spawn()
            .map_err(|error| AssetError::Worker(format!("could not start worker: {error}")))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| AssetError::Worker("worker stdin unavailable".to_owned()))?;
        write_frame(&mut stdin, &request)?;
        drop(stdin);
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| AssetError::Worker("worker stdout unavailable".to_owned()))?;
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let result = read_frame::<_, AssetWorkerResponse>(&mut stdout);
            let _ = sender.send(result);
        });
        let deadline = Instant::now() + self.timeout;
        let response = loop {
            if let Ok(response) = receiver.try_recv() {
                break response;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AssetError::Worker(
                    "worker timed out and was killed".to_owned(),
                ));
            }
            if let Some(status) = child
                .try_wait()
                .map_err(|error| AssetError::Worker(error.to_string()))?
                && !status.success()
            {
                let _ = child.wait();
                return Err(AssetError::Worker(format!(
                    "worker crashed or exited with {status}"
                )));
            }
            thread::sleep(Duration::from_millis(5));
        }?;
        let status = child
            .wait()
            .map_err(|error| AssetError::Worker(error.to_string()))?;
        if !status.success() {
            return Err(AssetError::Worker(format!("worker exited with {status}")));
        }
        if response.protocol_version != ASSET_WORKER_PROTOCOL_VERSION || response.token != token {
            return Err(AssetError::Worker(
                "worker response failed version/authentication validation".to_owned(),
            ));
        }
        response.artifact.ok_or_else(|| {
            AssetError::Worker(
                response
                    .error
                    .unwrap_or_else(|| "worker returned no artifact".to_owned()),
            )
        })
    }
}

fn preserve_minimal_environment(command: &mut Command) {
    for name in ["SYSTEMROOT", "WINDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

fn write_frame(mut writer: impl Write, value: &impl Serialize) -> Result<(), AssetError> {
    let payload = ron::ser::to_string(value)
        .map_err(|error| AssetError::Worker(error.to_string()))?
        .into_bytes();
    if payload.len() > MAX_WORKER_FRAME_BYTES {
        return Err(AssetError::Limit(format!(
            "worker frame exceeds {MAX_WORKER_FRAME_BYTES} bytes"
        )));
    }
    let length = u32::try_from(payload.len())
        .map_err(|_| AssetError::Limit("worker frame length exceeds u32".to_owned()))?;
    writer
        .write_all(&WORKER_MAGIC)
        .and_then(|()| writer.write_all(&ASSET_WORKER_PROTOCOL_VERSION.to_le_bytes()))
        .and_then(|()| writer.write_all(&length.to_le_bytes()))
        .and_then(|()| writer.write_all(&payload))
        .and_then(|()| writer.flush())
        .map_err(|error| AssetError::Worker(error.to_string()))
}

fn read_frame<R: Read, T: for<'de> Deserialize<'de>>(mut reader: R) -> Result<T, AssetError> {
    let mut header = [0_u8; 10];
    reader
        .read_exact(&mut header)
        .map_err(|error| AssetError::Worker(error.to_string()))?;
    if header[..4] != WORKER_MAGIC {
        return Err(AssetError::Worker("worker frame magic mismatch".to_owned()));
    }
    let version = u16::from_le_bytes([header[4], header[5]]);
    if version != ASSET_WORKER_PROTOCOL_VERSION {
        return Err(AssetError::Worker(format!(
            "worker frame version {version} is incompatible"
        )));
    }
    let length = u32::from_le_bytes([header[6], header[7], header[8], header[9]]) as usize;
    if length > MAX_WORKER_FRAME_BYTES {
        return Err(AssetError::Limit(format!(
            "worker frame length {length} exceeds policy"
        )));
    }
    let mut payload = vec![0; length];
    reader
        .read_exact(&mut payload)
        .map_err(|error| AssetError::Worker(error.to_string()))?;
    ron::de::from_bytes(&payload).map_err(|error| AssetError::Worker(error.to_string()))
}

/// Parses the worker token argument without accepting extra spellings.
pub fn worker_token_argument(arguments: impl IntoIterator<Item = OsString>) -> Option<Uuid> {
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--token" {
            return arguments
                .next()
                .and_then(|value| value.to_str().and_then(|text| Uuid::parse_str(text).ok()));
        }
    }
    None
}

/// Whether the worker stdio mode was explicitly requested.
pub fn worker_stdio_requested(arguments: impl IntoIterator<Item = OsString>) -> bool {
    arguments
        .into_iter()
        .any(|argument| argument == "--worker-stdio")
}

/// Canonical working-directory containment check for worker source paths.
///
/// # Errors
///
/// Returns an error when either path cannot be canonicalized or the source escapes `root`.
pub fn ensure_worker_path_contained(root: &Path, path: &Path) -> Result<(), AssetError> {
    let root = root
        .canonicalize()
        .map_err(|error| crate::io_error(root, error))?;
    let path = path
        .canonicalize()
        .map_err(|error| crate::io_error(path, error))?;
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(AssetError::UnsafePath(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn framed_protocol_rejects_oversized_and_bad_magic() {
        let mut bad = Vec::from(*b"BAD!");
        bad.extend_from_slice(&ASSET_WORKER_PROTOCOL_VERSION.to_le_bytes());
        bad.extend_from_slice(&0_u32.to_le_bytes());
        assert!(read_frame::<_, AssetWorkerResponse>(bad.as_slice()).is_err());

        let mut oversized = Vec::from(WORKER_MAGIC);
        oversized.extend_from_slice(&ASSET_WORKER_PROTOCOL_VERSION.to_le_bytes());
        oversized.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            read_frame::<_, AssetWorkerResponse>(oversized.as_slice()),
            Err(AssetError::Limit(_))
        ));
    }

    #[test]
    fn protocol_round_trip_preserves_auth_token() {
        let response = AssetWorkerResponse {
            protocol_version: ASSET_WORKER_PROTOCOL_VERSION,
            token: Uuid::new_v4(),
            artifact: None,
            error: Some("fixture".to_owned()),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &response).unwrap();
        let decoded: AssetWorkerResponse = read_frame(bytes.as_slice()).unwrap();
        assert_eq!(decoded.token, response.token);
        assert_eq!(decoded.error, response.error);
    }
}
