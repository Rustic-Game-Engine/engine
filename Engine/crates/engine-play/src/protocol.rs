//! Versioned, bounded wire protocol for editor/runtime communication.

use crate::simulation::{ControlAck, ControlRequest, PlayMode};
use crate::{changes::RuntimeChangeSet, frame_ring::BgraFrame};
use engine_core::ScriptId;
use engine_core::logging::{LogMetadata, LogRecord, Severity, SourceLocation};
use engine_world::{PartAttributes, ScriptComponent};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;
use uuid::Uuid;

/// Fixed prefix which prevents accidentally interpreting another local protocol.
pub const PROTOCOL_MAGIC: [u8; 8] = *b"RSTIPC01";
/// Absolute allocation bound for one control/log message.
pub const MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const HEADER_BYTES: usize = 34;

/// IPC compatibility generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

/// Protocol understood by this engine build.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 2, minor: 0 };

/// Current values for the selected entity while the isolated runtime is active.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveEntityProperties {
    pub entity_id: String,
    pub name: Option<String>,
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub part_attributes: PartAttributes,
    pub scripts: Vec<ScriptComponent>,
}

/// Process responsibility carried in every frame and authenticated handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRole {
    Editor,
    Runtime,
}

impl ProcessRole {
    const fn wire(self) -> u8 {
        match self {
            Self::Editor => 1,
            Self::Runtime => 2,
        }
    }

    fn from_wire(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Editor),
            2 => Ok(Self::Runtime),
            _ => Err(ProtocolError::InvalidRole(value)),
        }
    }
}

/// Random one-session secret. Debug output is deliberately redacted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AuthenticationToken([u8; 32]);

impl AuthenticationToken {
    /// Generates a fresh one-use token without depending on platform-specific APIs.
    pub fn generate() -> Self {
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let mut bytes = [0_u8; 32];
        bytes[..16].copy_from_slice(first.as_bytes());
        bytes[16..].copy_from_slice(second.as_bytes());
        Self(bytes)
    }

    /// Parses the exact 64-character hexadecimal environment representation.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::InvalidAuthenticationToken`] for malformed input.
    pub fn from_hex(value: &str) -> Result<Self, ProtocolError> {
        if value.len() != 64 {
            return Err(ProtocolError::InvalidAuthenticationToken);
        }
        let mut bytes = [0_u8; 32];
        let (pairs, remainder) = value.as_bytes().as_chunks::<2>();
        debug_assert!(remainder.is_empty());
        for (index, pair) in pairs.iter().enumerate() {
            let high = hex_digit(pair[0]).ok_or(ProtocolError::InvalidAuthenticationToken)?;
            let low = hex_digit(pair[1]).ok_or(ProtocolError::InvalidAuthenticationToken)?;
            bytes[index] = high << 4 | low;
        }
        Ok(Self(bytes))
    }

    /// Environment-safe representation used only at the spawn boundary.
    pub fn to_secret_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut output = String::with_capacity(64);
        for byte in self.0 {
            output.push(char::from(HEX[usize::from(byte >> 4)]));
            output.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        output
    }

    /// Constant-work comparison used during the handshake.
    pub fn securely_matches(&self, expected: &Self) -> bool {
        self.0
            .iter()
            .zip(expected.0.iter())
            .fold(0_u8, |difference, (left, right)| {
                difference | (left ^ right)
            })
            == 0
    }
}

impl fmt::Debug for AuthenticationToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("AuthenticationToken([REDACTED])")
    }
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// Stable process information attached to runtime console events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessDescriptor {
    pub role: ProcessRole,
    pub process_id: u32,
    pub name: String,
}

/// One structured event sent live to the editor console.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleEvent {
    pub process: ProcessDescriptor,
    pub record: ConsoleRecord,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stack_frames: Vec<SourceLocation>,
}

/// Wire-safe log record. Milliseconds use `i64` because RON rejects `i128`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsoleRecord {
    pub id: Uuid,
    pub timestamp_unix_millis: i64,
    pub severity: Severity,
    pub message: String,
    pub metadata: LogMetadata,
}

impl ConsoleRecord {
    pub fn from_log_record(record: &LogRecord) -> Self {
        let millis = record.timestamp_unix_nanos / 1_000_000;
        Self {
            id: record.id,
            timestamp_unix_millis: i64::try_from(millis).unwrap_or_else(|_| {
                if millis.is_negative() {
                    i64::MIN
                } else {
                    i64::MAX
                }
            }),
            severity: record.severity,
            message: record.message.clone(),
            metadata: record.metadata.clone(),
        }
    }
}

/// Versioned protocol payload. Every variant is explicitly bounded by frame decoding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "message", content = "data", rename_all = "snake_case")]
pub enum ProtocolMessage {
    ClientHello {
        token: AuthenticationToken,
        minimum_minor: u16,
    },
    ServerHello {
        negotiated: ProtocolVersion,
        process: ProcessDescriptor,
    },
    Ready {
        mode: PlayMode,
        fixed_tick: u64,
    },
    Control(ControlRequest),
    InputKeys(Vec<String>),
    QueryEntity(String),
    LiveEntity(Option<LiveEntityProperties>),
    ControlAck(ControlAck),
    Console(Box<ConsoleEvent>),
    Frame(BgraFrame),
    RuntimeChanges(RuntimeChangeSet),
    ReloadScript {
        format_version: u32,
        script_id: ScriptId,
        api_major: u16,
        source_sha256: String,
        source: Vec<u8>,
    },
    ReloadResult {
        accepted: bool,
        message: String,
    },
    Cancel {
        request_id: u64,
    },
    Cancelled {
        request_id: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

impl ProtocolMessage {
    const fn kind(&self) -> u8 {
        match self {
            Self::ClientHello { .. } => 1,
            Self::ServerHello { .. } => 2,
            Self::Ready { .. } => 3,
            Self::Control(_) => 4,
            Self::InputKeys(_) => 13,
            Self::QueryEntity(_) => 14,
            Self::LiveEntity(_) => 15,
            Self::ControlAck(_) => 5,
            Self::Console(_) => 6,
            Self::Frame(_) => 7,
            Self::RuntimeChanges(_) => 8,
            Self::ReloadScript { .. } => 11,
            Self::ReloadResult { .. } => 12,
            Self::Cancel { .. } => 9,
            Self::Cancelled { .. } => 10,
            Self::Error { .. } => 255,
        }
    }
}

/// Decoded frame metadata and payload.
#[derive(Debug, Clone, PartialEq)]
pub struct ReceivedMessage {
    pub version: ProtocolVersion,
    pub sender: ProcessRole,
    pub request_id: u64,
    pub deadline_unix_millis: u64,
    pub message: ProtocolMessage,
}

/// Wire validation, compatibility, framing, and I/O failures.
#[derive(Debug, Error)]
pub enum ProtocolError {
    #[error("IPC I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid IPC magic")]
    InvalidMagic,
    #[error("unsupported IPC protocol major {found}; expected {expected}")]
    IncompatibleMajor { found: u16, expected: u16 },
    #[error("IPC peer requires minor {minimum}, but this process supports {supported}")]
    IncompatibleMinor { minimum: u16, supported: u16 },
    #[error("invalid IPC process role {0}")]
    InvalidRole(u8),
    #[error("IPC payload length {found} exceeds bound {maximum}")]
    PayloadTooLarge { found: usize, maximum: usize },
    #[error("IPC payload could not be decoded: {0}")]
    Decode(String),
    #[error("IPC payload could not be encoded: {0}")]
    Encode(String),
    #[error("IPC frame kind {header} does not match decoded payload kind {payload}")]
    KindMismatch { header: u8, payload: u8 },
    #[error("IPC request deadline has expired")]
    DeadlineExpired,
    #[error("authentication token is malformed")]
    InvalidAuthenticationToken,
    #[error("IPC authentication failed")]
    AuthenticationFailed,
    #[error("the one-use IPC authentication token was already consumed")]
    AuthenticationReplay,
    #[error("unexpected IPC message: {0}")]
    UnexpectedMessage(&'static str),
}

/// Framed protocol connection over any reliable byte stream.
#[derive(Debug)]
pub struct IpcConnection<S> {
    stream: S,
    local_role: ProcessRole,
    negotiated: ProtocolVersion,
}

impl<S: Read + Write> IpcConnection<S> {
    pub fn new(stream: S, local_role: ProcessRole) -> Self {
        Self {
            stream,
            local_role,
            negotiated: PROTOCOL_VERSION,
        }
    }

    pub const fn negotiated_version(&self) -> ProtocolVersion {
        self.negotiated
    }

    pub fn set_negotiated_version(&mut self, version: ProtocolVersion) {
        self.negotiated = version;
    }

    /// Sends one message without an application deadline.
    ///
    /// # Errors
    ///
    /// Returns protocol encoding, size, or stream errors.
    pub fn send(
        &mut self,
        request_id: u64,
        message: &ProtocolMessage,
    ) -> Result<(), ProtocolError> {
        self.send_with_deadline(request_id, 0, message)
    }

    /// Sends a message with a Unix-millisecond deadline (`0` means none).
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::DeadlineExpired`] before writing an expired request.
    pub fn send_with_deadline(
        &mut self,
        request_id: u64,
        deadline_unix_millis: u64,
        message: &ProtocolMessage,
    ) -> Result<(), ProtocolError> {
        if deadline_unix_millis != 0 && deadline_unix_millis < unix_millis() {
            return Err(ProtocolError::DeadlineExpired);
        }
        let frame = encode_frame(
            self.local_role,
            self.negotiated,
            request_id,
            deadline_unix_millis,
            message,
        )?;
        self.stream.write_all(&frame)?;
        self.stream.flush()?;
        Ok(())
    }

    /// Receives and validates one complete frame before allocating its payload.
    ///
    /// # Errors
    ///
    /// Returns I/O, framing, size, compatibility, deadline, or decode errors.
    pub fn receive(&mut self) -> Result<ReceivedMessage, ProtocolError> {
        decode_frame(&mut self.stream)
    }

    pub fn into_inner(self) -> S {
        self.stream
    }

    pub(crate) const fn stream_ref(&self) -> &S {
        &self.stream
    }
}

fn encode_frame(
    sender: ProcessRole,
    version: ProtocolVersion,
    request_id: u64,
    deadline_unix_millis: u64,
    message: &ProtocolMessage,
) -> Result<Vec<u8>, ProtocolError> {
    let payload = if let ProtocolMessage::Frame(frame) = message {
        validate_pixel_frame(frame)?;
        let mut bytes = Vec::with_capacity(24 + frame.pixels.len());
        bytes.extend_from_slice(&frame.sequence.to_le_bytes());
        for value in [
            frame.width,
            frame.height,
            frame.stride_bytes,
            frame.pixels.len() as u32,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&frame.pixels);
        bytes
    } else {
        ron::ser::to_string(message)
            .map_err(|error| ProtocolError::Encode(error.to_string()))?
            .into_bytes()
    };
    if payload.len() > MAX_PAYLOAD_BYTES {
        return Err(ProtocolError::PayloadTooLarge {
            found: payload.len(),
            maximum: MAX_PAYLOAD_BYTES,
        });
    }
    let payload_length =
        u32::try_from(payload.len()).map_err(|_| ProtocolError::PayloadTooLarge {
            found: payload.len(),
            maximum: MAX_PAYLOAD_BYTES,
        })?;
    let mut frame = Vec::with_capacity(HEADER_BYTES + payload.len());
    frame.extend_from_slice(&PROTOCOL_MAGIC);
    frame.extend_from_slice(&version.major.to_le_bytes());
    frame.extend_from_slice(&version.minor.to_le_bytes());
    frame.push(sender.wire());
    frame.push(message.kind());
    frame.extend_from_slice(&request_id.to_le_bytes());
    frame.extend_from_slice(&deadline_unix_millis.to_le_bytes());
    frame.extend_from_slice(&payload_length.to_le_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn decode_frame(reader: &mut impl Read) -> Result<ReceivedMessage, ProtocolError> {
    let mut header = [0_u8; HEADER_BYTES];
    reader.read_exact(&mut header)?;
    if header[..8] != PROTOCOL_MAGIC {
        return Err(ProtocolError::InvalidMagic);
    }
    let major = u16::from_le_bytes([header[8], header[9]]);
    let minor = u16::from_le_bytes([header[10], header[11]]);
    if major != PROTOCOL_VERSION.major {
        return Err(ProtocolError::IncompatibleMajor {
            found: major,
            expected: PROTOCOL_VERSION.major,
        });
    }
    let sender = ProcessRole::from_wire(header[12])?;
    let kind = header[13];
    let request_id = u64::from_le_bytes(header[14..22].try_into().expect("fixed header range"));
    let deadline_unix_millis =
        u64::from_le_bytes(header[22..30].try_into().expect("fixed header range"));
    let payload_length =
        u32::from_le_bytes(header[30..34].try_into().expect("fixed header range")) as usize;
    if payload_length > MAX_PAYLOAD_BYTES {
        return Err(ProtocolError::PayloadTooLarge {
            found: payload_length,
            maximum: MAX_PAYLOAD_BYTES,
        });
    }
    if deadline_unix_millis != 0 && deadline_unix_millis < unix_millis() {
        return Err(ProtocolError::DeadlineExpired);
    }
    let mut payload = vec![0_u8; payload_length];
    reader.read_exact(&mut payload)?;
    let message: ProtocolMessage = if kind == 7 {
        if payload.len() < 24 {
            return Err(ProtocolError::Decode("truncated pixel frame".into()));
        }
        let u32_at = |offset| {
            u32::from_le_bytes(
                payload[offset..offset + 4]
                    .try_into()
                    .expect("validated pixel header"),
            )
        };
        let length = u32_at(20) as usize;
        if length != payload.len() - 24 {
            return Err(ProtocolError::Decode("pixel length mismatch".into()));
        }
        let frame = BgraFrame {
            sequence: u64::from_le_bytes(payload[..8].try_into().expect("validated pixel header")),
            width: u32_at(8),
            height: u32_at(12),
            stride_bytes: u32_at(16),
            pixels: payload[24..].to_vec(),
        };
        validate_pixel_frame(&frame)?;
        ProtocolMessage::Frame(frame)
    } else {
        let payload = std::str::from_utf8(&payload)
            .map_err(|error| ProtocolError::Decode(error.to_string()))?;
        ron::from_str(payload).map_err(|error| ProtocolError::Decode(error.to_string()))?
    };
    if message.kind() != kind {
        return Err(ProtocolError::KindMismatch {
            header: kind,
            payload: message.kind(),
        });
    }
    Ok(ReceivedMessage {
        version: ProtocolVersion { major, minor },
        sender,
        request_id,
        deadline_unix_millis,
        message,
    })
}

fn validate_pixel_frame(frame: &BgraFrame) -> Result<(), ProtocolError> {
    if frame.width == 0
        || frame.height == 0
        || frame.width > 640
        || frame.height > 360
        || frame.stride_bytes != frame.width * 4
        || frame.pixels.len() != (frame.stride_bytes * frame.height) as usize
    {
        return Err(ProtocolError::Decode(
            "invalid game frame dimensions or stride".into(),
        ));
    }
    Ok(())
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn authentication_token_round_trips_and_debug_is_redacted() {
        let token = AuthenticationToken::generate();
        let parsed = AuthenticationToken::from_hex(&token.to_secret_hex()).unwrap();
        assert!(parsed.securely_matches(&token));
        assert_eq!(format!("{token:?}"), "AuthenticationToken([REDACTED])");
        assert!(!format!("{token:?}").contains(&token.to_secret_hex()));
    }

    #[test]
    fn frame_round_trip_preserves_role_request_and_message() {
        let message = ProtocolMessage::Control(ControlRequest::Pause);
        let bytes = encode_frame(ProcessRole::Editor, PROTOCOL_VERSION, 42, 0, &message).unwrap();
        let decoded = decode_frame(&mut Cursor::new(bytes)).unwrap();
        assert_eq!(decoded.sender, ProcessRole::Editor);
        assert_eq!(decoded.request_id, 42);
        assert_eq!(decoded.message, message);
    }

    #[test]
    fn wrong_magic_and_major_are_rejected() {
        let message = ProtocolMessage::Control(ControlRequest::QueryState);
        let mut bad_magic =
            encode_frame(ProcessRole::Editor, PROTOCOL_VERSION, 1, 0, &message).unwrap();
        bad_magic[0] ^= 0xff;
        assert!(matches!(
            decode_frame(&mut Cursor::new(bad_magic)),
            Err(ProtocolError::InvalidMagic)
        ));

        let mut bad_major =
            encode_frame(ProcessRole::Editor, PROTOCOL_VERSION, 1, 0, &message).unwrap();
        bad_major[8..10].copy_from_slice(&99_u16.to_le_bytes());
        assert!(matches!(
            decode_frame(&mut Cursor::new(bad_major)),
            Err(ProtocolError::IncompatibleMajor { found: 99, .. })
        ));
    }

    #[test]
    fn oversized_length_is_rejected_before_payload_allocation() {
        let message = ProtocolMessage::Control(ControlRequest::QueryState);
        let mut bytes =
            encode_frame(ProcessRole::Editor, PROTOCOL_VERSION, 1, 0, &message).unwrap();
        let oversized = u32::try_from(MAX_PAYLOAD_BYTES + 1).unwrap();
        bytes[30..34].copy_from_slice(&oversized.to_le_bytes());
        assert!(matches!(
            decode_frame(&mut Cursor::new(bytes)),
            Err(ProtocolError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn truncated_payload_and_kind_mismatch_are_rejected() {
        let message = ProtocolMessage::Control(ControlRequest::Pause);
        let mut truncated =
            encode_frame(ProcessRole::Editor, PROTOCOL_VERSION, 1, 0, &message).unwrap();
        truncated.pop();
        assert!(matches!(
            decode_frame(&mut Cursor::new(truncated)),
            Err(ProtocolError::Io(_))
        ));

        let mut wrong_kind =
            encode_frame(ProcessRole::Editor, PROTOCOL_VERSION, 1, 0, &message).unwrap();
        wrong_kind[13] = 3;
        assert!(matches!(
            decode_frame(&mut Cursor::new(wrong_kind)),
            Err(ProtocolError::KindMismatch { .. })
        ));
    }

    #[test]
    fn expired_deadline_is_rejected() {
        let message = ProtocolMessage::Control(ControlRequest::Pause);
        let bytes = encode_frame(ProcessRole::Editor, PROTOCOL_VERSION, 1, 1, &message).unwrap();
        assert!(matches!(
            decode_frame(&mut Cursor::new(bytes)),
            Err(ProtocolError::DeadlineExpired)
        ));
    }
}

#[cfg(test)]
mod pixel_wire_tests {
    use super::*;
    #[test]
    fn full_game_frame_round_trip_is_bounded_and_exact() {
        let frame = BgraFrame {
            sequence: 12,
            width: 640,
            height: 360,
            stride_bytes: 2560,
            pixels: (0..921600).map(|i| (i % 256) as u8).collect(),
        };
        let message = ProtocolMessage::Frame(frame);
        let bytes = encode_frame(ProcessRole::Runtime, PROTOCOL_VERSION, 7, 0, &message).unwrap();
        assert_eq!(bytes.len(), HEADER_BYTES + 24 + 921600);
        assert!(bytes.len() < MAX_PAYLOAD_BYTES);
        assert_eq!(
            decode_frame(&mut std::io::Cursor::new(bytes))
                .unwrap()
                .message,
            message
        );
    }
    #[test]
    fn malformed_pixel_frame_is_rejected() {
        let frame = BgraFrame {
            sequence: 0,
            width: 640,
            height: 360,
            stride_bytes: 4,
            pixels: vec![0; 4],
        };
        assert!(
            encode_frame(
                ProcessRole::Runtime,
                PROTOCOL_VERSION,
                0,
                0,
                &ProtocolMessage::Frame(frame)
            )
            .is_err()
        );
    }
}
