//! Tauri commands: the bridge between the UI and the todoke-core engine.
//!
//! Patterns (per .scratch/gui/tauri-notes.md):
//! - async commands + `State<'_, tokio::sync::Mutex<..>>`, always `Result`.
//! - streaming send progress goes over a `tauri::ipc::Channel` (ordered,
//!   high-frequency); broadcasts (incoming receipts) use `app.emit`.
//! - engine calls never block the UI thread: everything runs on Tauri's
//!   tokio runtime.
//!
//! Commands are `pub` so `generate_handler![commands::name]` can reach the
//! generated dispatch macros.

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use base64::Engine as _;
use tauri::{AppHandle, Emitter, Manager, State, ipc::Channel};
use tokio::sync::watch;

use todoke_core::{
    discovery::{Advertiser, DeviceRecord},
    identity::DeviceIdentity,
    manifest::Receipt,
    transfer::{self, ReceiveOptions, SendOptions, SendReport, SenderInfo, TransferError},
    transport::{self, CertificateDer, ServerIdentity},
};

use crate::{
    dto::{
        DeviceDto, IdentityDto, IncomingDto, PathsStatsDto, ReceiptDto, ReceiveStatusDto, SendEvent,
    },
    state::{EngineMutex, EngineState, ReceiveHandle, SendHandle},
};

const DEFAULT_BROWSE_MS: u64 = 3000;
const MAX_BROWSE_MS: u64 = 15_000;

fn identity_dto(st: &EngineState, id: &DeviceIdentity) -> IdentityDto {
    IdentityDto {
        name: id.name().to_owned(),
        fingerprint: id.fingerprint(),
        data_dir: st.data_dir.display().to_string(),
        staging_dir: st.staging_dir.display().to_string(),
        default_target: st.target_dir.display().to_string(),
    }
}

fn receive_status_dto(st: &EngineState) -> ReceiveStatusDto {
    match &st.receive {
        Some(h) => ReceiveStatusDto {
            on: true,
            port: Some(h.port),
            target: Some(h.target.display().to_string()),
        },
        None => ReceiveStatusDto {
            on: false,
            port: None,
            target: None,
        },
    }
}

// ---------------------------------------------------------------- identity

#[tauri::command]
pub async fn get_identity(state: State<'_, EngineMutex>) -> Result<IdentityDto, String> {
    let st = state.lock().await;
    let id = st.identity()?;
    Ok(identity_dto(&st, id))
}

/// Rename this device. If receive mode is live, it is restarted so the new
/// name is on the network immediately.
#[tauri::command]
pub async fn set_device_name(
    state: State<'_, EngineMutex>,
    app: AppHandle,
    name: String,
) -> Result<IdentityDto, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("device name cannot be empty".into());
    }
    if name.chars().count() > 64 {
        return Err("device name is too long (max 64 characters)".into());
    }

    let mut st = state.lock().await;
    let was_serving = st.receive.is_some();
    let serving_target = st.receive.as_ref().map(|h| h.target.clone());
    if was_serving {
        stop_receive(&mut st).await;
    }
    {
        let fallback = st
            .identity_error
            .clone()
            .unwrap_or_else(|| "device identity is unavailable".into());
        let Some(id) = st.identity.as_mut() else {
            return Err(fallback);
        };
        id.set_name(name.clone())
            .map_err(|e| format!("rename failed: {e}"))?;
    }
    if was_serving {
        start_receive(&mut st, &app, serving_target).await?;
    }
    let id = st.identity()?;
    Ok(identity_dto(&st, id))
}

// --------------------------------------------------------------- discovery

#[tauri::command]
pub async fn list_devices(
    state: State<'_, EngineMutex>,
    timeout_ms: Option<u64>,
) -> Result<Vec<DeviceDto>, String> {
    let timeout = Duration::from_millis(
        timeout_ms
            .unwrap_or(DEFAULT_BROWSE_MS)
            .clamp(250, MAX_BROWSE_MS),
    );
    let own_fp = {
        let st = state.lock().await;
        st.identity.as_ref().map(|id| id.fingerprint())
    };
    let records: Vec<DeviceRecord> = todoke_core::discovery::browse(timeout)
        .await
        .map_err(|e| format!("discovery failed: {e}"))?;
    Ok(records
        .iter()
        // Don't surface this device to itself when receive mode is live.
        .filter(|r| Some(r.fingerprint.as_str()) != own_fp.as_deref())
        .map(DeviceDto::from)
        .collect())
}

// -------------------------------------------------------------------- send

/// Start sending `paths` to `addr`, streaming `SendEvent`s over `on_event`.
/// Returns the session id (for `cancel_transfer`). Completion arrives as a
/// `finished` (verified receipt) or `failed` event on the same channel,
/// strictly after all progress events.
#[tauri::command]
pub async fn send_files(
    state: State<'_, EngineMutex>,
    app: AppHandle,
    paths: Vec<String>,
    addr: String,
    fingerprint: String,
    cert_b64: String,
    on_event: Channel<SendEvent>,
) -> Result<u64, String> {
    if paths.is_empty() {
        return Err("no files selected".into());
    }
    let peer: SocketAddr = addr
        .parse()
        .map_err(|e| format!("invalid address '{addr}': {e}"))?;

    let (sender_name, sender_fp) = {
        let st = state.lock().await;
        let id = st.identity()?;
        (id.name().to_owned(), id.fingerprint())
    };
    let session = {
        let mut st = state.lock().await;
        st.next_session_id()
    };
    let (cancel_tx, cancel_rx) = watch::channel(false);

    tauri::async_runtime::spawn(run_send(
        app.clone(),
        session,
        peer,
        fingerprint,
        cert_b64,
        sender_name,
        sender_fp,
        paths,
        cancel_rx,
        on_event,
    ));

    let mut st = state.lock().await;
    st.sends.insert(session, SendHandle { cancel: cancel_tx });
    Ok(session)
}

/// The actual send session: dial, offer, stream, verify. Progress events are
/// pumped to the IPC channel in order; `finished`/`failed` always follow the
/// last progress event. Removes itself from the state map when done.
#[allow(clippy::too_many_arguments)]
async fn run_send(
    app: AppHandle,
    session: u64,
    peer: SocketAddr,
    fingerprint: String,
    cert_b64: String,
    sender_name: String,
    sender_fp: String,
    paths: Vec<String>,
    mut cancel_rx: watch::Receiver<bool>,
    on_event: Channel<SendEvent>,
) {
    // Progress pump: forwards engine events to the IPC channel in order and
    // ends once every sender is dropped (work finished or was cancelled).
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();
    let pump_channel = on_event.clone();
    let pump = tauri::async_runtime::spawn(async move {
        while let Some(ev) = event_rx.recv().await {
            let _ = pump_channel.send(SendEvent::from(ev));
        }
    });

    let sender = SenderInfo {
        name: sender_name,
        fingerprint: sender_fp,
    };
    let work = async {
        let der = base64::engine::general_purpose::STANDARD
            .decode(&cert_b64)
            .map_err(|e| format!("the destination advertised a corrupt certificate: {e}"))?;
        let cert = CertificateDer::from(der);
        let (_endpoint, conn) = transport::dial(peer, &fingerprint, cert)
            .await
            .map_err(|e| e.to_string())?;
        let opts = SendOptions {
            parallelism: 4,
            progress: Some(event_tx),
        };
        transfer::send_with(&conn, &paths, &sender, &opts)
            .await
            .map_err(|e| e.to_string())
    };

    let outcome: Result<SendReport, String> = tokio::select! {
        r = work => r,
        // Dropping `work` here drops the connection mid-session; the
        // receiver keeps its staging, so a retry resumes.
        _ = cancel_rx.wait_for(|cancelled| *cancelled) => Err("cancelled".into()),
    };

    // All progress events are on the wire before we report the outcome.
    let _ = pump.await;
    match outcome {
        Ok(report) => {
            let _ = on_event.send(SendEvent::Finished {
                receipt: ReceiptDto::from(&report.receipt),
            });
        }
        Err(message) => {
            let _ = on_event.send(SendEvent::Failed { message });
        }
    }

    let st = app.state::<EngineMutex>();
    st.lock().await.sends.remove(&session);
}

/// Gracefully stop a send session: the connection drops, the receiver keeps
/// its staging, and a retry resumes where this session left off.
#[tauri::command]
pub async fn cancel_transfer(state: State<'_, EngineMutex>, session: u64) -> Result<(), String> {
    let mut st = state.lock().await;
    match st.sends.remove(&session) {
        Some(handle) => {
            let _ = handle.cancel.send(true);
            Ok(())
        }
        None => Err(format!("no active send session {session}")),
    }
}

// ----------------------------------------------------------------- receive

/// Bind the QUIC endpoint and start advertising this device on the LAN.
async fn start_receive(
    st: &mut EngineState,
    app: &AppHandle,
    target: Option<PathBuf>,
) -> Result<(), String> {
    if st.receive.is_some() {
        return Ok(());
    }
    let target = target.unwrap_or_else(|| st.target_dir.clone());
    // Fail before advertising if we cannot land files (CLI ensure_target rule).
    std::fs::create_dir_all(&target)
        .map_err(|e| format!("target dir {}: {e}", target.display()))?;

    let identity = st.identity()?;
    let server_identity: ServerIdentity =
        transport::server_identity(identity).map_err(|e| format!("identity certificate: {e}"))?;
    let addr: SocketAddr = "0.0.0.0:0".parse().expect("valid wildcard socket");
    let (endpoint, local) = transport::serve(&server_identity, addr)
        .await
        .map_err(|e| format!("cannot bind receive endpoint: {e}"))?;
    let port = local.port();
    let advertiser = Advertiser::advertise(identity, &server_identity, port).map_err(|e| {
        // endpoint drops here, unbinding the port
        format!(
            "cannot advertise on this network: {e} (on Windows the firewall may block mDNS for unsigned apps)"
        )
    })?;

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let opts = ReceiveOptions::new(target.clone(), st.staging_dir.clone());
    let accept_task = tauri::async_runtime::spawn(receive_loop(
        endpoint.clone(),
        shutdown_rx,
        app.clone(),
        opts,
    ));

    st.receive = Some(ReceiveHandle {
        endpoint,
        advertiser,
        shutdown: shutdown_tx,
        accept_task,
        port,
        target,
    });
    Ok(())
}

/// Stop receive mode: end the accept loop, close the endpoint, unregister
/// the advertisement. In-flight transfers see a peer-visible connection
/// close; their staging is kept (resumable).
async fn stop_receive(st: &mut EngineState) {
    if let Some(handle) = st.receive.take() {
        let _ = handle.shutdown.send(true);
        handle.endpoint.close(2u32.into(), b"todoke stopping");
        let _ = tokio::time::timeout(Duration::from_secs(2), handle.accept_task).await;
        let _ = handle.advertiser.stop();
    }
}

async fn receive_loop(
    endpoint: quinn::Endpoint,
    mut shutdown: watch::Receiver<bool>,
    app: AppHandle,
    opts: ReceiveOptions,
) {
    loop {
        let incoming = tokio::select! {
            inc = endpoint.accept() => inc,
            _ = shutdown.wait_for(|on| *on) => break,
        };
        let Some(incoming) = incoming else { break };
        let Ok(conn) = incoming.await else { continue };

        let app = app.clone();
        let opts = opts.clone();
        tauri::async_runtime::spawn(async move {
            let from = conn.remote_address().to_string();
            let result: Result<Receipt, TransferError> = transfer::receive(&conn, &opts).await;
            let _ = app.emit(
                "receive://incoming",
                IncomingDto::from_result(from, &result),
            );
        });
    }
}

#[tauri::command]
pub async fn receive_start(
    state: State<'_, EngineMutex>,
    app: AppHandle,
    target: Option<String>,
) -> Result<ReceiveStatusDto, String> {
    let mut st = state.lock().await;
    start_receive(&mut st, &app, target.map(PathBuf::from)).await?;
    Ok(receive_status_dto(&st))
}

#[tauri::command]
pub async fn receive_stop(state: State<'_, EngineMutex>) -> Result<ReceiveStatusDto, String> {
    let mut st = state.lock().await;
    stop_receive(&mut st).await;
    Ok(receive_status_dto(&st))
}

#[tauri::command]
pub async fn receive_status(state: State<'_, EngineMutex>) -> Result<ReceiveStatusDto, String> {
    let st = state.lock().await;
    Ok(receive_status_dto(&st))
}

// -------------------------------------------------------------- filesystem

/// Default receive target for the picker's initial directory.
#[tauri::command]
pub async fn get_receive_target(state: State<'_, EngineMutex>) -> Result<String, String> {
    let st = state.lock().await;
    Ok(st.target_dir.display().to_string())
}

#[tauri::command]
pub async fn set_receive_target(
    state: State<'_, EngineMutex>,
    target: String,
) -> Result<ReceiveStatusDto, String> {
    let mut st = state.lock().await;
    if st.receive.is_some() {
        return Err("stop receive mode before changing the target folder".into());
    }
    let path = PathBuf::from(&target);
    std::fs::create_dir_all(&path).map_err(|e| format!("target dir {}: {e}", path.display()))?;
    st.target_dir = path;
    Ok(receive_status_dto(&st))
}

/// Recursive size/count summary of send inputs (dirs walked like the
/// manifest builder would). Best effort per entry: unreadable files are
/// reported in `skipped` instead of failing the whole call. Runs on the
/// blocking pool — large trees must not stall an async worker.
#[tauri::command]
pub async fn stat_paths(paths: Vec<String>) -> Result<PathsStatsDto, String> {
    if paths.is_empty() {
        return Err("no paths given".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let mut files: u64 = 0;
        let mut bytes: u64 = 0;
        let mut skipped: u64 = 0;
        fn walk(
            p: &std::path::Path,
            files: &mut u64,
            bytes: &mut u64,
            skipped: &mut u64,
            depth: u32,
        ) {
            if depth > 64 {
                *skipped += 1;
                return;
            }
            let meta = match std::fs::symlink_metadata(p) {
                Ok(m) => m,
                Err(_) => {
                    *skipped += 1;
                    return;
                }
            };
            if meta.is_dir() {
                let Ok(entries) = std::fs::read_dir(p) else {
                    *skipped += 1;
                    return;
                };
                for entry in entries.flatten() {
                    walk(&entry.path(), files, bytes, skipped, depth + 1);
                }
            } else {
                *files += 1;
                *bytes += meta.len();
            }
        }
        for raw in &paths {
            walk(
                std::path::Path::new(raw),
                &mut files,
                &mut bytes,
                &mut skipped,
                0,
            );
        }
        Ok(PathsStatsDto {
            files,
            total_bytes: bytes,
            skipped,
        })
    })
    .await
    .map_err(|e| format!("stat task failed: {e}"))?
}
