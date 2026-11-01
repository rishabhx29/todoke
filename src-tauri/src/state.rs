//! Long-lived engine state behind `State<'_, tokio::sync::Mutex<EngineState>>`.
//!
//! tokio Mutex (not std) because every command here is async; the identity is
//! mutable (rename) and shared with the receive loop's spawned tasks, so the
//! lock guards all of it.

use std::{collections::HashMap, path::PathBuf};

use todoke_core::{discovery::Advertiser, identity::DeviceIdentity};

pub type EngineMutex = tokio::sync::Mutex<EngineState>;

/// Where received files are finalized by default: Documents/Todoke when the
/// OS exposes it, otherwise <data>/incoming.
fn default_target(data_dir: &std::path::Path) -> PathBuf {
    dirs::document_dir()
        .map(|d| d.join("Todoke"))
        .unwrap_or_else(|| data_dir.join("incoming"))
}

pub fn data_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("TODOKE_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    dirs::data_dir()
        .map(|d| d.join("todoke"))
        .ok_or_else(|| "could not locate the app-data directory".to_string())
}

/// One active send session (cancellable via its cancel flag).
pub struct SendHandle {
    pub cancel: tokio::sync::watch::Sender<bool>,
}

/// Live receive mode: the QUIC endpoint, the mDNS advertiser, and the accept
/// loop. Dropping unregisters the advertisement; `shutdown` ends the loop.
pub struct ReceiveHandle {
    pub endpoint: quinn::Endpoint,
    pub advertiser: Advertiser,
    pub shutdown: tokio::sync::watch::Sender<bool>,
    pub accept_task: tauri::async_runtime::JoinHandle<()>,
    pub port: u16,
    pub target: PathBuf,
}

pub struct EngineState {
    pub identity: Option<DeviceIdentity>,
    pub identity_error: Option<String>,
    pub data_dir: PathBuf,
    pub staging_dir: PathBuf,
    pub target_dir: PathBuf,
    pub receive: Option<ReceiveHandle>,
    pub sends: HashMap<u64, SendHandle>,
    next_session: u64,
}

impl EngineState {
    pub fn new(
        identity: Option<DeviceIdentity>,
        identity_error: Option<String>,
        data_dir: PathBuf,
    ) -> Self {
        let staging_dir = data_dir.join("staging");
        let target_dir = default_target(&data_dir);
        Self {
            identity,
            identity_error,
            data_dir,
            staging_dir,
            target_dir,
            receive: None,
            sends: HashMap::new(),
            next_session: 1,
        }
    }

    /// Allocate the next send-session id.
    pub fn next_session_id(&mut self) -> u64 {
        let id = self.next_session;
        self.next_session += 1;
        id
    }

    /// Identity access that fails with a UI-presentable message.
    pub fn identity(&self) -> Result<&DeviceIdentity, String> {
        self.identity.as_ref().ok_or_else(|| {
            self.identity_error
                .clone()
                .unwrap_or_else(|| "device identity is unavailable".into())
        })
    }
}
