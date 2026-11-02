//! Transfer sessions: the sender and receiver state machines that move a
//! manifest's bytes over any [`Transport`], stage everything on the receiving
//! side, and finalize files into the target as each one completes — closed
//! out by a receipt the sender checks against its own manifest before
//! reporting success.
//!
//! Parallelism: the sender stripes chunks across N concurrent data streams
//! (default 4, bounded by the receiver's Accept); the receiver processes the
//! streams concurrently and finalizes each file the moment its last chunk
//! verifies. Canceling or losing a session mid-way therefore leaves
//! already-completed files finalized in the target, the rest staged
//! (resumable, ticket 07), and the target never holds partial files.
//!
//! Staging layout (receiver): `<staging_root>/<manifest hash prefix>/<file
//! relative path>` — keyed by manifest hash so a later session for the same
//! content can reconcile against whatever is already there.

use std::{
    collections::{HashSet, VecDeque},
    fs,
    future::Future,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::mpsc::UnboundedSender;

use crate::{
    chunk::{ChunkParams, hash_file},
    manifest::{Manifest, Receipt, ReceiptFile, build_manifest_and_paths},
    protocol::{
        ChunkId, Envelope, HaveFile, Message, ProtocolError, read_data_frame, read_message,
        write_data_frame, write_message,
    },
    transport::Transport,
};

/// How much of a manifest hash keys a staging directory (full hashes are
/// long for filesystem paths; 16 hex chars = 64 bits is collision-safe for
/// any realistic number of concurrent manifests).
const STAGING_KEY_LEN: usize = 16;

/// Maximum Missing/re-request rounds before giving up on a session.
const MAX_MISSING_ROUNDS: u32 = 3;

/// Maximum staging directories kept per receiver (evicted oldest-first at
/// session start, so aborted manifests can't grow staging unbounded).
const MAX_STAGING_DIRS: usize = 8;

/// How long each side waits for the peer's FIN during quiescence before
/// giving up (a rogue peer must not hang us).
const QUIESCE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error("I/O error during transfer: {0}")]
    Io(#[from] std::io::Error),
    #[error("protocol error: {0}")]
    Protocol(#[from] ProtocolError),
    #[error("manifest error: {0}")]
    Manifest(#[from] crate::manifest::ManifestError),
    #[error("chunking/hashing error: {0}")]
    Chunk(#[from] crate::chunk::ChunkError),
    #[error("transport error: {0}")]
    Transport(#[from] crate::transport::TransportError),
    #[error("unexpected message at this point in the session: {0:?}")]
    UnexpectedMessage(Message),
    #[error("peer reported an error: {0}")]
    RemoteError(String),
    #[error("peer referenced a different manifest ({0})")]
    WrongManifest(String),
    #[error(
        "chunk {chunk} of '{path}' failed its hash check: expected {expected}, got {got}; transfer aborted before anything was finalized"
    )]
    ChunkHashMismatch {
        path: String,
        chunk: usize,
        expected: String,
        got: String,
    },
    #[error("file '{path}' failed final verification: expected hash {expected}, got {got}")]
    FileHashMismatch {
        path: String,
        expected: String,
        got: String,
    },
    #[error("file '{path}' is {staged} bytes staged but should be {expected}")]
    SizeMismatch {
        path: String,
        staged: u64,
        expected: u64,
    },
    #[error("data frame referenced file {0}, which is not in the manifest")]
    UnknownFile(u32),
    #[error("data frame referenced chunk {chunk} of file {file}, beyond that file's chunk list")]
    UnknownChunk { file: u32, chunk: u32 },
    #[error("file '{path}' was incomplete when the sender declared the session complete")]
    IncompleteAtComplete { path: String },
    #[error("the receipt returned by the receiver did not match the manifest we sent")]
    ReceiptMismatch,
    #[error("peer cancelled the session: {0}")]
    Cancelled(String),
    #[error("connection lost mid-transfer: {0}")]
    ConnectionLost(String),
}

/// Who is sending: carried in the protocol hello.
#[derive(Debug, Clone)]
pub struct SenderInfo {
    pub name: String,
    pub fingerprint: String,
}

/// What a successful send produced: the offered manifest plus the receipt
/// the receiver returned (already verified against the manifest).
#[derive(Debug, Clone)]
pub struct SendReport {
    pub manifest: Manifest,
    pub receipt: Receipt,
}

/// Live progress events from a send session. Subscribe by providing an
/// unbounded channel sender in [`SendOptions`]; the GUI/CLI consumes these
/// without polling.
#[derive(Debug, Clone)]
pub enum TransferEvent {
    /// A file with bytes to send entered the work queue.
    FileStarted { path: String, size: u64 },
    /// One chunk was queued onto a data stream.
    ChunkSent {
        path: String,
        chunk: u32,
        bytes: u64,
    },
    /// Every chunk of a file has been sent.
    FileSent { path: String },
}

/// Sender-side session configuration.
#[derive(Debug, Clone)]
pub struct SendOptions {
    /// Concurrent data streams (bounded by the receiver's Accept).
    pub parallelism: usize,
    /// Optional progress event sink.
    pub progress: Option<UnboundedSender<TransferEvent>>,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self {
            parallelism: 4,
            progress: None,
        }
    }
}

impl SendOptions {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Receiver-side session configuration.
#[derive(Debug, Clone)]
pub struct ReceiveOptions {
    /// Where verified files are finalized into (never touched before
    /// their own verification completes).
    pub target_dir: PathBuf,
    /// Root directory for in-progress staging state, keyed by manifest hash.
    pub staging_root: PathBuf,
    /// How many data streams the receiver accepts (advertised in Accept).
    pub parallelism: usize,
    /// Optional session-stats sink (peak concurrent data streams).
    pub stats: Option<UnboundedSender<SessionStats>>,
}

impl ReceiveOptions {
    pub fn new(target_dir: impl Into<PathBuf>, staging_root: impl Into<PathBuf>) -> Self {
        Self {
            target_dir: target_dir.into(),
            staging_root: staging_root.into(),
            parallelism: 4,
            stats: None,
        }
    }
}

/// Receiver-side session statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionStats {
    /// Highest number of data streams open at the same time.
    pub peak_streams: usize,
}

/// Send files/directories over `transport`, returning the verified receipt.
///
/// The manifest is built with the default chunk parameters (the single
/// source of truth in [`crate::chunk`]). Any error aborts the session;
/// already-completed files stay finalized on the receiver, and its staging
/// keeps what it already wrote so a retry can reconcile.
pub async fn send<T: Transport>(
    transport: &T,
    inputs: &[impl AsRef<Path>],
    sender: &SenderInfo,
) -> Result<SendReport, TransferError> {
    send_with(transport, inputs, sender, &SendOptions::new()).await
}

/// [`send`] with explicit parallelism and progress subscription.
pub async fn send_with<T: Transport>(
    transport: &T,
    inputs: &[impl AsRef<Path>],
    sender: &SenderInfo,
    options: &SendOptions,
) -> Result<SendReport, TransferError> {
    let unit = build_manifest_and_paths(inputs, &ChunkParams::default())?;
    let manifest = unit.manifest;

    let (mut ctl_tx, mut ctl_rx) = transport.open_bi().await?;
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Hello {
            name: sender.name.clone(),
            fingerprint: sender.fingerprint.clone(),
        }),
    )
    .await?;
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Offer {
            manifest: manifest.clone(),
        }),
    )
    .await?;

    let (receiver_streams, have) = match read_message(&mut ctl_rx).await? {
        Envelope {
            message:
                Message::Accept {
                    manifest_hash,
                    have,
                    streams,
                },
            ..
        } => {
            if manifest_hash != manifest.hash {
                return Err(TransferError::WrongManifest(manifest_hash));
            }
            (streams.max(1) as usize, have)
        }
        Envelope {
            message: Message::Error { message },
            ..
        } => return Err(TransferError::RemoteError(message)),
        Envelope {
            message: Message::Cancel { reason },
            ..
        } => return Err(TransferError::Cancelled(reason)),
        Envelope { message, .. } => return Err(TransferError::UnexpectedMessage(message)),
    };

    // What the receiver already has (empty on a fresh transfer): skip those
    // chunks. Resume (ticket 07) feeds this from persisted state.
    let skip: Vec<HashSet<u32>> = {
        let mut sets = vec![HashSet::new(); manifest.files.len()];
        for HaveFile { file, chunks } in have {
            if let Some(set) = sets.get_mut(file as usize) {
                set.extend(chunks);
            }
        }
        sets
    };

    // Flatten the outstanding work: (file, chunk) pairs in stable order.
    let mut tasks: VecDeque<(u32, u32)> = VecDeque::new();
    for (file_idx, entry) in manifest.files.iter().enumerate() {
        for chunk_idx in 0..entry.chunks.len() {
            if entry.chunks.is_empty() || skip[file_idx].contains(&(chunk_idx as u32)) {
                continue;
            }
            tasks.push_back((file_idx as u32, chunk_idx as u32));
        }
    }
    // What to send this round: everything the receiver doesn't already have.
    let send_sets: Vec<HashSet<u32>> = manifest
        .files
        .iter()
        .enumerate()
        .map(|(file_idx, entry)| {
            entry
                .chunks
                .iter()
                .enumerate()
                .filter(|(ci, _)| !skip[file_idx].contains(&(*ci as u32)))
                .map(|(ci, _)| ci as u32)
                .collect()
        })
        .collect();

    run_round(
        transport,
        &manifest,
        &unit.sources,
        options.progress.as_ref(),
        send_sets,
        receiver_streams,
        options.parallelism,
        true,
    )
    .await?;

    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Complete {
            manifest_hash: manifest.hash.clone(),
        }),
    )
    .await?;

    // The receiver verifies everything it staged; if chunks arrived damaged
    // it answers Missing instead of Receipt and we resend exactly those.
    let mut rounds = 0u32;
    loop {
        match read_message(&mut ctl_rx).await? {
            Envelope {
                message: Message::Receipt { receipt },
                ..
            } => {
                if !receipt.verify_against(&manifest) {
                    return Err(TransferError::ReceiptMismatch);
                }
                quiesce(&mut ctl_tx, &mut ctl_rx).await;
                return Ok(SendReport { manifest, receipt });
            }
            Envelope {
                message: Message::Missing { chunks, .. },
                ..
            } => {
                rounds += 1;
                if rounds > MAX_MISSING_ROUNDS {
                    return Err(TransferError::IncompleteAtComplete {
                        path: chunks
                            .first()
                            .map(|_| "multiple files (missing after retries)".into())
                            .unwrap_or_default(),
                    });
                }
                // Resend exactly the missing chunks, in manifest order.
                let mut per_file: Vec<HashSet<u32>> = vec![HashSet::new(); manifest.files.len()];
                for ChunkId { file, chunk } in &chunks {
                    if let Some(set) = per_file.get_mut(*file as usize) {
                        set.insert(*chunk);
                    }
                }
                run_round(
                    transport,
                    &manifest,
                    &unit.sources,
                    options.progress.as_ref(),
                    per_file,
                    receiver_streams,
                    options.parallelism,
                    false,
                )
                .await?;
                write_message(
                    &mut ctl_tx,
                    &Envelope::new(Message::Complete {
                        manifest_hash: manifest.hash.clone(),
                    }),
                )
                .await?;
            }
            Envelope {
                message: Message::Error { message },
                ..
            } => return Err(TransferError::RemoteError(message)),
            Envelope {
                message: Message::Cancel { reason },
                ..
            } => return Err(TransferError::Cancelled(reason)),
            Envelope { message, .. } => return Err(TransferError::UnexpectedMessage(message)),
        }
    }
}

/// One data-stream round: stripe the given per-file chunk sets across N
/// streams. Used for the initial send and for Missing re-requests.
#[allow(clippy::too_many_arguments)]
async fn run_round<T: Transport>(
    transport: &T,
    manifest: &Manifest,
    sources: &[PathBuf],
    progress: Option<&UnboundedSender<TransferEvent>>,
    send_sets: Vec<HashSet<u32>>,
    receiver_streams: usize,
    parallelism: usize,
    emit_file_started: bool,
) -> Result<(), TransferError> {
    let mut tasks: VecDeque<(u32, u32)> = VecDeque::new();
    for (file_idx, entry) in manifest.files.iter().enumerate() {
        let send_set = send_sets.get(file_idx).cloned().unwrap_or_default();
        if entry.chunks.is_empty() {
            continue;
        }
        if !send_set.is_empty() && emit_file_started {
            if let Some(progress) = progress {
                let _ = progress.send(TransferEvent::FileStarted {
                    path: entry.path.clone(),
                    size: entry.size,
                });
            }
        }
        for chunk_idx in 0..entry.chunks.len() {
            if !send_set.contains(&(chunk_idx as u32)) {
                continue;
            }
            tasks.push_back((file_idx as u32, chunk_idx as u32));
        }
    }
    let total_tasks = tasks.len();
    let queue = Arc::new(Mutex::new(tasks));
    let n_streams = receiver_streams
        .min(parallelism.max(1))
        .clamp(1, total_tasks.max(1));

    let manifest_ref = manifest;
    let sources_ref = sources;
    let workers = (0..n_streams).map(|_| {
        let queue = Arc::clone(&queue);
        let progress = progress.cloned();
        async move {
            let (mut data_tx, _data_rx) = transport.open_bi().await?;
            // Per-worker state: handles stay open while a worker works through
            // a file's chunks (queue order is contiguous per file), and one
            // read buffer is reused for every chunk instead of a fresh
            // allocation each time.
            let mut handles: std::collections::HashMap<u32, tokio::fs::File> =
                std::collections::HashMap::new();
            let mut buf = Vec::with_capacity(256 * 1024);
            loop {
                let task = {
                    let mut q = queue.lock().expect("task queue poisoned");
                    q.pop_front()
                };
                let Some((file_idx, chunk_idx)) = task else {
                    break;
                };
                let entry = &manifest_ref.files[file_idx as usize];
                let chunk = &entry.chunks[chunk_idx as usize];
                let file = match handles.entry(file_idx) {
                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                    std::collections::hash_map::Entry::Vacant(e) => {
                        e.insert(tokio::fs::File::open(&sources_ref[file_idx as usize]).await?)
                    }
                };
                buf.resize(chunk.length as usize, 0);
                file.seek(SeekFrom::Start(chunk.offset)).await?;
                file.read_exact(&mut buf).await?;
                write_data_frame(&mut data_tx, file_idx, chunk_idx, &buf).await?;
                if let Some(progress) = &progress {
                    let _ = progress.send(TransferEvent::ChunkSent {
                        path: entry.path.clone(),
                        chunk: chunk_idx,
                        bytes: buf.len() as u64,
                    });
                    if chunk_idx as usize + 1 == entry.chunks.len() {
                        let _ = progress.send(TransferEvent::FileSent {
                            path: entry.path.clone(),
                        });
                    }
                }
            }
            data_tx
                .finish()
                .map_err(|e| TransferError::ConnectionLost(e.to_string()))?;
            Ok::<(), TransferError>(())
        }
    });
    futures::future::try_join_all(workers).await?;
    Ok(())
}

/// Receive one transfer session on `transport`, staging under
/// `opts.staging_root` and finalizing each file into `opts.target_dir` as
/// soon as it fully verifies. Returns the receipt it sent back.
pub async fn receive<T: Transport>(
    transport: &T,
    opts: &ReceiveOptions,
) -> Result<Receipt, TransferError> {
    use futures::StreamExt as _;
    let (mut ctl_tx, mut ctl_rx) = transport.accept_bi().await?;

    match read_message(&mut ctl_rx).await? {
        Envelope {
            message: Message::Hello { .. },
            ..
        } => {}
        Envelope {
            message: Message::Cancel { reason },
            ..
        } => return Err(TransferError::Cancelled(reason)),
        Envelope {
            message: Message::Error { message },
            ..
        } => return Err(TransferError::RemoteError(message)),
        Envelope { message, .. } => return Err(TransferError::UnexpectedMessage(message)),
    }

    let manifest = match read_message(&mut ctl_rx).await? {
        Envelope {
            message: Message::Offer { manifest },
            ..
        } => manifest,
        Envelope {
            message: Message::Cancel { reason },
            ..
        } => return Err(TransferError::Cancelled(reason)),
        Envelope { message, .. } => return Err(TransferError::UnexpectedMessage(message)),
    };
    if !manifest.verify_hash() {
        let detail = "offered manifest failed its own hash verification".to_string();
        let _ = write_message(
            &mut ctl_tx,
            &Envelope::new(Message::Error {
                message: detail.clone(),
            }),
        )
        .await;
        return Err(TransferError::RemoteError(detail));
    }

    let stage_dir = staging_dir(&opts.staging_root, &manifest);
    let staging_key: String = manifest.hash.chars().take(STAGING_KEY_LEN).collect();
    sweep_staging(&opts.staging_root, &staging_key);
    fs::create_dir_all(&stage_dir)?;
    // Pre-create every directory the manifest needs — the frame loop then
    // never pays a create_dir_all per chunk.
    {
        use std::collections::HashSet as DirSet;
        let mut made: DirSet<PathBuf> = DirSet::new();
        for entry in &manifest.files {
            let staged = stage_dir.join(&entry.path);
            if let Some(parent) = staged.parent() {
                if made.insert(parent.to_path_buf()) {
                    fs::create_dir_all(parent)?;
                }
            }
        }
    }

    // Resume (ticket 07): reconcile the staged leftovers from any previous
    // session against the manifest — chunks whose bytes hash correctly on
    // disk are already here and will not be re-sent. This lives on disk, so
    // it survives crashes and restarts on both sides.
    let disk_have = reconcile_from_disk(&manifest, &stage_dir);

    // Session state shared by the per-stream processors, seeded with what
    // the disk already holds; arriving frames extend it.
    let state = Arc::new(Mutex::new(SessionState {
        received: disk_have.clone(),
        last_bad: None,
        active_streams: 0,
        peak_streams: 0,
    }));

    let accept_streams = opts.parallelism.max(1);
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Accept {
            manifest_hash: manifest.hash.clone(),
            have: have_list(&disk_have),
            streams: accept_streams as u32,
        }),
    )
    .await?;

    // One future per accepted stream: each accepts, processes frames until
    // FIN, and records what it saw. Any stream error aborts the session.
    // Borrowed futures joined inline — the caller's connection outlives them.
    let manifest_ref = &manifest;
    let stage_dir_ref = &stage_dir;

    // Concurrent session loop: accept data streams as the sender opens them
    // (up to the advertised count), process frames on each, and watch the
    // control stream — the sender may legitimately open fewer streams than
    // advertised, so NOTHING here may block on the stream count alone.
    type ProcessorFuture<'a> =
        std::pin::Pin<Box<dyn Future<Output = Result<(), TransferError>> + Send + 'a>>;
    let mut finalized: std::collections::HashMap<usize, ReceiptFile> =
        std::collections::HashMap::new();
    let mut rounds = 0u32;

    /// Why the concurrent session loop ended.
    enum Outcome {
        Complete,
        Cancelled(String),
        PeerError(String),
        Unexpected(Message),
        SessionFailed(TransferError),
    }
    // Rounds loop: each round runs the concurrent session (accept streams,
    // process frames, watch control) and ends in a receipt, an error, or a
    // Missing re-request that starts the next round.
    loop {
        let mut processors: futures::stream::FuturesUnordered<ProcessorFuture<'_>> =
            futures::stream::FuturesUnordered::new();
        let mut accepted = 0usize;
        let mut finished = 0usize;
        let mut outcome: Option<Outcome> = None;
        let loop_result: Result<(), TransferError> = loop {
            if outcome.is_some() {
                break Ok(());
            }
            tokio::select! {
                // A data stream ended (FIN or error).
                ended = processors.next(), if finished < accepted => {
                    match ended {
                        Some(Ok(())) => finished += 1,
                        Some(Err(e)) => outcome = Some(Outcome::SessionFailed(e)),
                        None => finished = accepted, // all done
                    }
                }
                // The sender opened another data stream.
                pair = transport.accept_bi(), if accepted < accept_streams => {
                    let pair = match pair {
                        Ok(p) => p,
                        Err(e) => {
                            outcome = Some(Outcome::SessionFailed(e.into()));
                            continue;
                        }
                    };
                    let (data_tx, mut data_rx) = pair;
                    let _ = data_tx; // data streams carry receiver->sender nothing
                    {
                        let mut st = state.lock().expect("session state poisoned");
                        st.active_streams += 1;
                        st.peak_streams = st.peak_streams.max(st.active_streams);
                    }
                    accepted += 1;
                    let state = Arc::clone(&state);
                    // This stream's own file handles: opened once per file it
                    // touches, written at chunk offsets, closed when the stream
                    // ends. Handles are per-stream, so nothing races the
                    // finalization pass (which runs after every stream settled).
                    let mut handles: std::collections::HashMap<u32, fs::File> =
                        std::collections::HashMap::new();
                    processors.push(Box::pin(async move {
                        let result = loop {
                            let (file, chunk, payload) = match read_data_frame(&mut data_rx).await {
                                Ok(Some(frame)) => frame,
                                Ok(None) => break Ok(()), // clean FIN
                                Err(ProtocolError::Io(e)) => break Err(TransferError::Io(e)),
                                Err(e) => break Err(e.into()),
                            };
                            if let Err(e) = stage_frame(
                                manifest_ref,
                                &state,
                                stage_dir_ref,
                                &mut handles,
                                file,
                                chunk,
                                &payload,
                            ) {
                                break Err(e);
                            }
                        };
                        drop(handles);
                        {
                            let mut st = state.lock().expect("session state poisoned");
                            st.active_streams = st.active_streams.saturating_sub(1);
                        }
                        result
                    }));
                }
                // Control stream: session-level messages.
                ctl = read_message(&mut ctl_rx) => {
                    match ctl {
                        Ok(Envelope { message: Message::Complete { manifest_hash }, .. }) => {
                            if manifest_hash == manifest.hash {
                                outcome = Some(Outcome::Complete);
                            } else {
                                outcome = Some(Outcome::PeerError(format!(
                                    "sender completed a different manifest ({manifest_hash})"
                                )));
                            }
                        }
                        Ok(Envelope { message: Message::Error { message }, .. }) => {
                            outcome = Some(Outcome::PeerError(message));
                        }
                        Ok(Envelope { message: Message::Cancel { reason }, .. }) => {
                            // Cancellation keeps fully-received files (finalized
                            // below) and staging (resumable).
                            outcome = Some(Outcome::Cancelled(reason));
                        }
                        Ok(Envelope { message, .. }) => {
                            outcome = Some(Outcome::Unexpected(message));
                        }
                        Err(e) => {
                            outcome = Some(Outcome::SessionFailed(e.into()));
                        }
                    }
                }
            }
        };

        // Drain remaining processors (FINs may still be in flight), bounded so a
        // misbehaving sender cannot hang us past the quiesce budget. Stream
        // errors are captured, not fatal: files that already arrived whole still
        // get finalized below.
        let mut stream_error: Option<TransferError> = None;
        let drain = async {
            while let Some(result) = processors.next().await {
                if let Err(e) = result {
                    stream_error.get_or_insert(e);
                }
            }
            Ok::<(), TransferError>(())
        };
        if tokio::time::timeout(QUIESCE_TIMEOUT, drain).await.is_err() {
            stream_error.get_or_insert(TransferError::ConnectionLost(
                "data streams did not settle".into(),
            ));
        }

        // What is still missing after this round's frames settled (received sets
        // include the disk reconciliation from session start).
        let (peak, last_bad, received_snapshot) = {
            let st = state.lock().expect("session state poisoned");
            (st.peak_streams, st.last_bad.clone(), st.received.clone())
        };
        let mut missing: Vec<ChunkId> = Vec::new();
        for (file_idx, entry) in manifest.files.iter().enumerate() {
            if entry.chunks.is_empty() {
                continue;
            }
            for chunk_idx in 0..entry.chunks.len() {
                if !received_snapshot[file_idx].contains(&(chunk_idx as u32)) {
                    missing.push(ChunkId {
                        file: file_idx as u32,
                        chunk: chunk_idx as u32,
                    });
                }
            }
        }

        // Single-threaded finalization pass — runs on EVERY exit path so files
        // that arrived whole are kept (cancel/loss semantics: completed files
        // stay finalized, the rest stays staged for resume). No streams are
        // alive here, so no staging handles can race the moves. Files finalized
        // in earlier rounds are skipped (their staging is already moved out).
        for (file_idx, entry) in manifest.files.iter().enumerate() {
            if finalized.contains_key(&file_idx) {
                continue;
            }
            let got_all = received_snapshot[file_idx].len() == entry.chunks.len();
            if !got_all && entry.size > 0 {
                continue; // genuinely incomplete: stays staged (resumable)
            }
            let staged = stage_dir.join(&entry.path);
            if entry.size == 0 && !staged.exists() {
                if let Some(parent) = staged.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&staged, b"")?;
            }
            let staged_size = fs::metadata(&staged)?.len();
            if staged_size != entry.size {
                return Err(TransferError::SizeMismatch {
                    path: entry.path.clone(),
                    staged: staged_size,
                    expected: entry.size,
                });
            }
            // Whole-file verification of everything that reached disk. Large
            // files hash on all cores (mmap + rayon); the result is identical
            // to the single-threaded hash either way.
            let got = hash_file(&staged)?;
            if got != entry.hash {
                return Err(TransferError::FileHashMismatch {
                    path: entry.path.clone(),
                    expected: entry.hash.clone(),
                    got,
                });
            }
            let dest = collision_free_path(&opts.target_dir.join(&entry.path))?;
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent)?;
            }
            move_into_place(&staged, &dest)?;
            finalized.insert(
                file_idx,
                ReceiptFile {
                    path: entry.path.clone(),
                    size: entry.size,
                    hash: entry.hash.clone(),
                    chunks: entry.chunks.len() as u64,
                },
            );
        }

        // Stats: the peak is final once every stream settled.
        if let Some(stats) = &opts.stats {
            let _ = stats.send(SessionStats { peak_streams: peak });
        }

        // Outcome mapping for this round.
        let outcome = match (outcome, loop_result) {
            (Some(o), _) => o,
            (None, Err(e)) => Outcome::SessionFailed(e),
            (None, Ok(())) => Outcome::Complete,
        };
        match outcome {
            Outcome::Complete if missing.is_empty() => {
                fs::create_dir_all(&opts.target_dir)?;
                for dir in &manifest.empty_dirs {
                    fs::create_dir_all(opts.target_dir.join(dir))?;
                }
                let receipt = Receipt {
                    manifest_hash: manifest.hash.clone(),
                    files: manifest
                        .files
                        .iter()
                        .enumerate()
                        .map(|(i, _)| finalized.remove(&i).expect("all files finalized"))
                        .collect(),
                };
                write_message(
                    &mut ctl_tx,
                    &Envelope::new(Message::Receipt {
                        receipt: receipt.clone(),
                    }),
                )
                .await?;
                quiesce(&mut ctl_tx, &mut ctl_rx).await;
                let _ = fs::remove_dir_all(&stage_dir);
                return Ok(receipt);
            }
            Outcome::Complete => {
                // Something is missing: ask for exactly those chunks and run
                // another concurrent round (bounded).
                rounds += 1;
                if rounds > MAX_MISSING_ROUNDS {
                    if let Some(bad) = last_bad {
                        return Err(TransferError::ChunkHashMismatch {
                            path: bad.path,
                            chunk: bad.chunk,
                            expected: bad.expected,
                            got: bad.got,
                        });
                    }
                    let path = manifest
                        .files
                        .iter()
                        .zip(&received_snapshot)
                        .find(|(e, r)| r.len() < e.chunks.len())
                        .map(|(e, _)| e.path.clone())
                        .unwrap_or_default();
                    return Err(TransferError::IncompleteAtComplete { path });
                }
                write_message(
                    &mut ctl_tx,
                    &Envelope::new(Message::Missing {
                        manifest_hash: manifest.hash.clone(),
                        chunks: missing,
                    }),
                )
                .await?;
                processors.clear();
                // Falls through to the next round of this loop.
            }
            Outcome::Cancelled(reason) => return Err(TransferError::Cancelled(reason)),
            Outcome::PeerError(message) => return Err(TransferError::RemoteError(message)),
            Outcome::Unexpected(message) => return Err(TransferError::UnexpectedMessage(message)),
            Outcome::SessionFailed(e) => {
                // A chunk-level hash failure is the most diagnostic cause.
                if let Some(bad) = last_bad {
                    return Err(TransferError::ChunkHashMismatch {
                        path: bad.path,
                        chunk: bad.chunk,
                        expected: bad.expected,
                        got: bad.got,
                    });
                }
                return Err(stream_error.unwrap_or(e));
            }
        }
    }
}

/// Rebuild the have-list from what is actually on disk in the staging dir:
/// a chunk counts as had only if its staged bytes still hash correctly, so
/// crashes and truncation are handled without a separate state file.
fn reconcile_from_disk(manifest: &Manifest, stage_dir: &Path) -> Vec<HashSet<u32>> {
    let mut valid: Vec<HashSet<u32>> = manifest.files.iter().map(|_| HashSet::new()).collect();
    for entry in &manifest.files {
        let staged = stage_dir.join(&entry.path);
        if let Some(parent) = staged.parent() {
            let _ = fs::create_dir_all(parent);
        }
    }
    for (file_idx, entry) in manifest.files.iter().enumerate() {
        if entry.chunks.is_empty() {
            continue; // empty files are trivially complete
        }
        let staged = stage_dir.join(&entry.path);
        let Ok(mut f) = fs::File::open(&staged) else {
            continue;
        };
        // A partially-staged file is expected on resume: judge each chunk
        // individually (present AND hash-correct), never the whole file size.
        let len = f.metadata().map_or(0, |m| m.len());
        for (ci, chunk) in entry.chunks.iter().enumerate() {
            if chunk.offset.saturating_add(chunk.length) > len {
                continue; // past what ever reached disk
            }
            let mut buf = vec![0u8; chunk.length as usize];
            use std::io::Read as _;
            if f.seek(SeekFrom::Start(chunk.offset)).is_err() {
                continue;
            }
            if f.read_exact(&mut buf).is_err() {
                continue;
            }
            if blake3::hash(&buf).to_hex().to_string() == chunk.hash {
                valid[file_idx].insert(ci as u32);
            }
        }
    }
    valid
}

/// HaveFile list for the Accept message (only files with anything to report).
fn have_list(valid: &[HashSet<u32>]) -> Vec<HaveFile> {
    valid
        .iter()
        .enumerate()
        .filter(|(_, set)| !set.is_empty())
        .map(|(file_idx, set)| HaveFile {
            file: file_idx as u32,
            chunks: set.iter().copied().collect(),
        })
        .collect()
}

/// Bound the staging root: keep the newest MAX_STAGING_DIRS directories
/// (excluding the current session's), evicting oldest-first, so aborted
/// manifests cannot grow it unbounded.
fn sweep_staging(staging_root: &Path, keep_key: &str) {
    let Ok(entries) = fs::read_dir(staging_root) else {
        return;
    };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let modified = e.metadata().and_then(|m| m.modified()).ok()?;
            Some((modified, e.path()))
        })
        .filter(|(_, p)| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy() != keep_key)
        })
        .collect();
    if dirs.len() < MAX_STAGING_DIRS {
        return;
    }
    dirs.sort_by_key(|(t, _)| *t);
    let excess = dirs.len() + 1 - MAX_STAGING_DIRS;
    for (_, path) in dirs.into_iter().take(excess) {
        let _ = fs::remove_dir_all(path);
    }
}

/// Staging directory for one manifest under the staging root.
fn staging_dir(staging_root: &Path, manifest: &Manifest) -> PathBuf {
    let key: String = manifest
        .hash
        .chars()
        .take(STAGING_KEY_LEN)
        .collect::<String>()
        .to_lowercase();
    staging_root.join(key)
}

/// A chunk that arrived but failed its hash check.
#[derive(Debug, Clone)]
struct BadChunk {
    path: String,
    chunk: usize,
    expected: String,
    got: String,
}

/// Shared session state for the per-stream processors: what each file has
/// received so far (seeded from disk for resume). Finalization happens once
/// per round, after all streams settle, so no other state is shared.
struct SessionState {
    received: Vec<HashSet<u32>>,
    last_bad: Option<BadChunk>,
    active_streams: usize,
    peak_streams: usize,
}

/// Handle one data frame: validate, verify, stage. Finalization is a
/// separate pass (after all streams settle) so nothing races the moves.
fn stage_frame(
    manifest: &Manifest,
    state: &Arc<Mutex<SessionState>>,
    stage_dir: &Path,
    handles: &mut std::collections::HashMap<u32, fs::File>,
    file: u32,
    chunk: u32,
    payload: &[u8],
) -> Result<(), TransferError> {
    let entry = manifest
        .files
        .get(file as usize)
        .ok_or(TransferError::UnknownFile(file))?;
    let chunk_info = entry
        .chunks
        .get(chunk as usize)
        .ok_or(TransferError::UnknownChunk { file, chunk })?;
    if payload.len() as u64 != chunk_info.length {
        return Err(TransferError::SizeMismatch {
            path: entry.path.clone(),
            staged: payload.len() as u64,
            expected: chunk_info.length,
        });
    }
    let got = blake3::hash(payload).to_hex().to_string();
    if got != chunk_info.hash {
        // Damaged in transit: record it, write nothing. The reconciliation
        // after the streams settle reports it as missing and the sender
        // re-requests via the Missing/Complete round loop.
        let mut st = state.lock().expect("session state poisoned");
        st.last_bad.get_or_insert(BadChunk {
            path: entry.path.clone(),
            chunk: chunk as usize,
            expected: chunk_info.hash.clone(),
            got,
        });
        return Ok(());
    }

    let mut st = state.lock().expect("session state poisoned");
    if st.received[file as usize].insert(chunk) {
        // Parent dirs were pre-created at session start (resume included:
        // reconcile re-runs this pass), so frames only write.
        let staged = stage_dir.join(&entry.path);
        // Offset-based chunk writes: never truncate — resume (ticket 07)
        // relies on previously staged bytes surviving.
        let out = handles.entry(file).or_insert_with(|| {
            fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&staged)
                .expect("staging file open")
        });
        out.seek(SeekFrom::Start(chunk_info.offset))?;
        out.write_all(payload)?;
    }
    Ok(())
}

/// FIN our control side and drain the peer's, bounded — both sides must drop
/// the connection only after the last control message was delivered (an
/// implicit close discards queued-but-unacked stream data). The control
/// streams are quinn streams by construction of [`crate::transport`].
async fn quiesce(ctl_tx: &mut quinn::SendStream, ctl_rx: &mut quinn::RecvStream) {
    let _ = ctl_tx.finish();
    let mut drain = [0u8; 64];
    let _ = tokio::time::timeout(QUIESCE_TIMEOUT, async {
        loop {
            match ctl_rx.read(&mut drain).await {
                Ok(None) | Err(_) => break,
                Ok(Some(_)) => {}
            }
        }
    })
    .await;
}

/// `name (2).ext`-style deterministic disambiguation. Existing files are
/// never overwritten.
fn collision_free_path(dest: &Path) -> Result<PathBuf, TransferError> {
    if !dest.exists() {
        return Ok(dest.to_path_buf());
    }
    let parent = dest
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let stem = dest
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = dest
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()));
    for n in 2u32..=10_000 {
        let candidate = parent.join(format!("{stem} ({n}){}", ext.as_deref().unwrap_or("")));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(TransferError::Io(std::io::Error::other(format!(
        "could not find a free name for '{}': too many collisions",
        dest.display()
    ))))
}

/// Rename when possible (same volume: instant), copy+delete otherwise
/// (staging on a different drive from the target).
fn move_into_place(src: &Path, dest: &Path) -> Result<(), TransferError> {
    if fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    fs::copy(src, dest)?;
    fs::remove_file(src)?;
    Ok(())
}

/// Count staged bytes under a staging directory (test/diagnostics helper).
#[must_use]
pub fn staged_bytes(staging_root: &Path, manifest_hash: &str) -> u64 {
    let key: String = manifest_hash
        .chars()
        .take(STAGING_KEY_LEN)
        .collect::<String>()
        .to_lowercase();
    let dir = staging_root.join(key);
    let mut total = 0;
    let Ok(entries) = fs::read_dir(&dir) else {
        return 0;
    };
    let mut stack: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            if let Ok(entries) = fs::read_dir(&path) {
                stack.extend(entries.filter_map(Result::ok).map(|e| e.path()));
            }
        } else if let Ok(md) = path.metadata() {
            total += md.len();
        }
    }
    total
}
