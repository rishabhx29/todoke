//! serde DTOs crossing the IPC boundary.
//!
//! Naming rules (see .scratch/gui/tauri-notes.md): payloads camelCase via
//! serde; the streaming enum is internally tagged `{ event, data }` so the
//! frontend can discriminate on `event`.

use serde::{Deserialize, Serialize};

/// This device's identity as shown in the UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityDto {
    pub name: String,
    pub fingerprint: String,
    pub data_dir: String,
    pub staging_dir: String,
    pub default_target: String,
}

/// A nearby device discovered over mDNS.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDto {
    pub name: String,
    pub fingerprint: String,
    /// Certificate DER (base64) to pin at connect time.
    pub cert_b64: String,
    pub addr: String,
    pub port: u16,
}

impl From<&todoke_core::discovery::DeviceRecord> for DeviceDto {
    fn from(r: &todoke_core::discovery::DeviceRecord) -> Self {
        Self {
            name: r.name.clone(),
            fingerprint: r.fingerprint.clone(),
            cert_b64: r.cert_b64.clone(),
            addr: r.addr.to_string(),
            port: r.port,
        }
    }
}

/// Per-file line of a verification receipt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptFileDto {
    pub path: String,
    pub size: u64,
    pub hash: String,
    pub chunks: u64,
}

/// Verified end-to-end receipt (manifest hash + per-file hashes).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptDto {
    pub manifest_hash: String,
    pub files: Vec<ReceiptFileDto>,
}

impl From<&todoke_core::manifest::Receipt> for ReceiptDto {
    fn from(r: &todoke_core::manifest::Receipt) -> Self {
        Self {
            manifest_hash: r.manifest_hash.clone(),
            files: r
                .files
                .iter()
                .map(|f| ReceiptFileDto {
                    path: f.path.clone(),
                    size: f.size,
                    hash: f.hash.clone(),
                    chunks: f.chunks,
                })
                .collect(),
        }
    }
}

/// Streaming progress from a send session, pushed over a
/// `tauri::ipc::Channel` (ordered, high-frequency — the documented pattern
/// for progress streams).
#[derive(Debug, Clone, Serialize)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "event",
    content = "data"
)]
pub enum SendEvent {
    FileStarted {
        path: String,
        size: u64,
    },
    ChunkSent {
        path: String,
        chunk: u32,
        bytes: u64,
    },
    FileSent {
        path: String,
    },
    Finished {
        receipt: ReceiptDto,
    },
    Failed {
        message: String,
    },
}

impl From<todoke_core::transfer::TransferEvent> for SendEvent {
    fn from(e: todoke_core::transfer::TransferEvent) -> Self {
        use todoke_core::transfer::TransferEvent;
        match e {
            TransferEvent::FileStarted { path, size } => SendEvent::FileStarted { path, size },
            TransferEvent::ChunkSent { path, chunk, bytes } => {
                SendEvent::ChunkSent { path, chunk, bytes }
            }
            TransferEvent::FileSent { path } => SendEvent::FileSent { path },
        }
    }
}

/// One entry of the incoming-transfers log (receive side), broadcast with
/// `app.emit("receive://incoming", ...)`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingDto {
    pub from_addr: String,
    pub manifest_hash: String,
    pub files: Vec<ReceiptFileDto>,
    pub total_bytes: u64,
    pub ok: bool,
    pub error: Option<String>,
    pub at_unix_ms: u64,
}

impl IncomingDto {
    pub fn from_result(
        from_addr: String,
        result: &Result<todoke_core::manifest::Receipt, todoke_core::transfer::TransferError>,
    ) -> Self {
        let at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        match result {
            Ok(receipt) => Self {
                from_addr,
                manifest_hash: receipt.manifest_hash.clone(),
                files: ReceiptDto::from(receipt).files,
                total_bytes: receipt.total_bytes(),
                ok: true,
                error: None,
                at_unix_ms,
            },
            Err(e) => Self {
                from_addr,
                manifest_hash: String::new(),
                files: Vec::new(),
                total_bytes: 0,
                ok: false,
                error: Some(e.to_string()),
                at_unix_ms,
            },
        }
    }
}

/// Whether receive mode is live, and on what port/target.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveStatusDto {
    pub on: bool,
    pub port: Option<u16>,
    pub target: Option<String>,
}

/// Size/count summary of send inputs (`stat_paths`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathsStatsDto {
    pub files: u64,
    pub total_bytes: u64,
    pub skipped: u64,
}
