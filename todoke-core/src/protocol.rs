//! The Todoke transfer protocol: length-prefixed JSON control messages in a
//! versioned envelope, plus compact binary framing for chunk data.
//!
//! Wire format:
//! - Control message: `u32 BE length` + JSON `{"v":1,"m":{"type":"...",...}}`.
//! - Data frame (on data streams): `u8 version` + `u32 BE file index` +
//!   `u32 BE chunk index` + `u64 BE payload length` + payload bytes.
//!
//! Unknown message types and unknown versions are rejected cleanly (typed
//! errors, connection closed with a reason) so two different Todoke versions
//! fail loudly instead of misinterpreting each other.

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::manifest::{Manifest, Receipt};

/// Current wire protocol version.
pub const PROTOCOL_VERSION: u32 = 1;

/// Upper bound for one control message (manifests of very large batches fit;
/// anything beyond this is a protocol violation).
pub const MAX_CONTROL_MESSAGE: u32 = 64 * 1024 * 1024;

/// Upper bound for one data-frame payload (chunks max out at 256 KiB; the
/// bound exists to catch desynchronized streams early).
pub const MAX_DATA_PAYLOAD: u64 = 8 * 1024 * 1024;

/// Data-frame layout version byte.
pub const DATA_FRAME_VERSION: u8 = 1;

#[derive(Debug, thiserror::Error)]
pub enum ProtocolError {
    #[error("I/O error on protocol stream: {0}")]
    Io(#[from] std::io::Error),
    #[error("message encoding error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("control message of {0} bytes exceeds the {MAX_CONTROL_MESSAGE}-byte limit")]
    MessageTooLarge(u32),
    #[error("peer speaks protocol version {0}, we speak {PROTOCOL_VERSION}")]
    UnsupportedVersion(u32),
    #[error("peer sent an unknown message type (different Todoke version?)")]
    UnknownMessage,
    #[error("data frame version {0} is not supported")]
    FrameVersion(u8),
    #[error("data frame payload of {0} bytes exceeds the {MAX_DATA_PAYLOAD}-byte limit")]
    FrameTooLarge(u64),
}

/// A versioned control-message envelope.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Envelope {
    #[serde(rename = "v")]
    pub version: u32,
    #[serde(rename = "m")]
    pub message: Message,
}

impl Envelope {
    pub fn new(message: Message) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            message,
        }
    }
}

/// What the receiver already has, per file (used to reconcile on connect
/// and resume). Chunk indices refer to the manifest's chunk order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HaveFile {
    pub file: u32,
    pub chunks: Vec<u32>,
}

/// One chunk reference in a reconcile/re-request list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChunkId {
    pub file: u32,
    pub chunk: u32,
}

/// Control vocabulary.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    /// Identity exchange opening a session.
    Hello { name: String, fingerprint: String },
    /// Sender offers a manifest.
    Offer { manifest: Manifest },
    /// Receiver accepts and reports what it already has (resume).
    ///
    /// `streams` is how many data streams the receiver will accept for this
    /// session; the sender opens `min(its own parallelism, streams)` of them.
    Accept {
        manifest_hash: String,
        have: Vec<HaveFile>,
        streams: u32,
    },
    /// Sender believes everything outstanding was sent.
    Complete { manifest_hash: String },
    /// Receiver disagrees: these chunks were never (validly) received.
    Missing {
        manifest_hash: String,
        chunks: Vec<ChunkId>,
    },
    /// Receiver finalized everything and verified it against the manifest.
    Receipt { receipt: Receipt },
    /// Either side cancels; the session stays resumable.
    Cancel { reason: String },
    /// Terminal error report.
    Error { message: String },
    /// Any unknown `type` tag lands here during deserialization so `read`
    /// can reject it explicitly instead of failing opaquely.
    #[serde(other)]
    Unknown,
}

/// Write one length-prefixed JSON control message.
pub async fn write_message<W: AsyncWrite + Unpin>(
    w: &mut W,
    envelope: &Envelope,
) -> Result<(), ProtocolError> {
    let bytes = serde_json::to_vec(envelope)?;
    let len = u32::try_from(bytes.len()).map_err(|_| ProtocolError::MessageTooLarge(u32::MAX))?;
    if len > MAX_CONTROL_MESSAGE {
        return Err(ProtocolError::MessageTooLarge(len));
    }
    w.write_all(&len.to_be_bytes()).await?;
    w.write_all(&bytes).await?;
    Ok(())
}

/// Read one length-prefixed JSON control message, enforcing the protocol
/// version and rejecting unknown message types.
pub async fn read_message<R: AsyncRead + Unpin>(r: &mut R) -> Result<Envelope, ProtocolError> {
    let mut len_bytes = [0u8; 4];
    r.read_exact(&mut len_bytes).await?;
    let len = u32::from_be_bytes(len_bytes);
    if len == 0 || len > MAX_CONTROL_MESSAGE {
        return Err(ProtocolError::MessageTooLarge(len));
    }
    let mut buf = vec![0u8; len as usize];
    r.read_exact(&mut buf).await?;
    let envelope: Envelope = serde_json::from_slice(&buf)?;
    if envelope.version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(envelope.version));
    }
    if matches!(envelope.message, Message::Unknown) {
        return Err(ProtocolError::UnknownMessage);
    }
    Ok(envelope)
}

/// Write one binary data frame (chunk payload) to a data stream.
pub async fn write_data_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    file: u32,
    chunk: u32,
    payload: &[u8],
) -> Result<(), ProtocolError> {
    let len = u64::try_from(payload.len()).unwrap_or(u64::MAX);
    if len > MAX_DATA_PAYLOAD {
        return Err(ProtocolError::FrameTooLarge(len));
    }
    let mut header = [0u8; 1 + 4 + 4 + 8];
    header[0] = DATA_FRAME_VERSION;
    header[1..5].copy_from_slice(&file.to_be_bytes());
    header[5..9].copy_from_slice(&chunk.to_be_bytes());
    header[9..17].copy_from_slice(&len.to_be_bytes());
    w.write_all(&header).await?;
    w.write_all(payload).await?;
    Ok(())
}

/// Read one binary data frame, returning `Some((file, chunk, payload))`.
///
/// `Ok(None)` marks a clean end of stream (the peer finished its sending
/// side before beginning a new frame); a frame cut off mid-header or
/// mid-payload surfaces as an I/O error.
pub async fn read_data_frame<R: AsyncRead + Unpin>(
    r: &mut R,
) -> Result<Option<(u32, u32, Vec<u8>)>, ProtocolError> {
    let mut version = [0u8; 1];
    if r.read(&mut version).await? == 0 {
        return Ok(None);
    }
    if version[0] != DATA_FRAME_VERSION {
        return Err(ProtocolError::FrameVersion(version[0]));
    }
    let mut rest = [0u8; 4 + 4 + 8];
    r.read_exact(&mut rest).await?;
    let file = u32::from_be_bytes(rest[0..4].try_into().unwrap_or([0; 4]));
    let chunk = u32::from_be_bytes(rest[4..8].try_into().unwrap_or([0; 4]));
    let len = u64::from_be_bytes(rest[8..16].try_into().unwrap_or([0; 8]));
    if len > MAX_DATA_PAYLOAD {
        return Err(ProtocolError::FrameTooLarge(len));
    }
    let mut payload = vec![0u8; len as usize];
    r.read_exact(&mut payload).await?;
    Ok(Some((file, chunk, payload)))
}
