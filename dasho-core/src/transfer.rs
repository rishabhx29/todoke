//! Transfer sessions: the sender and receiver state machines that move a
//! manifest's bytes over any [`Transport`], stage everything on the receiving
//! side, and finalize into the target only after verification — closed out by
//! a receipt the sender checks against its own manifest before reporting
//! success.
//!
//! Ticket 05 scope: one control stream plus one data stream (the
//! single-stream path). Parallel multi-stream striping (ticket 06) and
//! persistent have-lists / resume (ticket 07) build on the same protocol
//! vocabulary and staging layout.
//!
//! Staging layout (receiver): `<staging_root>/<manifest hash prefix>/<file
//! relative path>` — keyed by manifest hash so a later session for the same
//! content can reconcile against whatever is already there. The target
//! directory is only ever written by finalization, which runs after every
//! file has passed whole-file verification.

use std::{
    collections::HashSet,
    fs,
    io::{ErrorKind, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::{
    chunk::{ChunkParams, hash_reader},
    manifest::{Manifest, Receipt, ReceiptFile, build_manifest_and_paths},
    protocol::{
        Envelope, HaveFile, Message, ProtocolError, read_data_frame, read_message,
        write_data_frame, write_message,
    },
    transport::Transport,
};

/// How much of a manifest hash keys a staging directory (full hashes are
/// long for filesystem paths; 16 hex chars = 64 bits is collision-safe for
/// any realistic number of concurrent manifests).
const STAGING_KEY_LEN: usize = 16;

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

/// Receiver-side session configuration.
#[derive(Debug, Clone)]
pub struct ReceiveOptions {
    /// Where verified files are finalized into (never touched before
    /// verification completes).
    pub target_dir: PathBuf,
    /// Root directory for in-progress staging state, keyed by manifest hash.
    pub staging_root: PathBuf,
}

impl ReceiveOptions {
    pub fn new(target_dir: impl Into<PathBuf>, staging_root: impl Into<PathBuf>) -> Self {
        Self {
            target_dir: target_dir.into(),
            staging_root: staging_root.into(),
        }
    }
}

/// Send files/directories over `transport`, returning the verified receipt.
///
/// The manifest is built with the default chunk parameters (the single
/// source of truth in [`crate::chunk`]). Any error aborts the session; the
/// receiver's staging keeps what it already wrote, so a retry can reconcile.
pub async fn send<T: Transport>(
    transport: &T,
    inputs: &[impl AsRef<Path>],
    sender: &SenderInfo,
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

    let have = match read_message(&mut ctl_rx).await? {
        Envelope {
            message:
                Message::Accept {
                    manifest_hash,
                    have,
                },
            ..
        } => {
            if manifest_hash != manifest.hash {
                return Err(TransferError::WrongManifest(manifest_hash));
            }
            have
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
    let mut skip: Vec<HashSet<u32>> = vec![HashSet::new(); manifest.files.len()];
    for HaveFile { file, chunks } in have {
        if let Some(set) = skip.get_mut(file as usize) {
            set.extend(chunks);
        }
    }

    let (mut data_tx, _data_rx) = transport.open_bi().await?;
    for (file_idx, (entry, source)) in manifest.files.iter().zip(&unit.sources).enumerate() {
        if entry.chunks.is_empty() {
            continue; // empty files have no bytes to send
        }
        let mut file = tokio::fs::File::open(source).await?;
        for (chunk_idx, chunk) in entry.chunks.iter().enumerate() {
            if skip[file_idx].contains(&(chunk_idx as u32)) {
                continue;
            }
            let mut buf = vec![0u8; chunk.length as usize];
            file.seek(SeekFrom::Start(chunk.offset)).await?;
            file.read_exact(&mut buf).await?;
            write_data_frame(&mut data_tx, file_idx as u32, chunk_idx as u32, &buf).await?;
        }
    }
    // FIN on the data stream tells the receiver the frame stream is over.
    data_tx
        .finish()
        .map_err(|e| TransferError::ConnectionLost(e.to_string()))?;

    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Complete {
            manifest_hash: manifest.hash.clone(),
        }),
    )
    .await?;

    match read_message(&mut ctl_rx).await? {
        Envelope {
            message: Message::Receipt { receipt },
            ..
        } => {
            if !receipt.verify_against(&manifest) {
                return Err(TransferError::ReceiptMismatch);
            }
            // Quiesce: FIN our control side and drain the peer's, so both
            // sides drop the connection only after the Receipt was delivered.
            let _ = ctl_tx.finish();
            let mut drain = [0u8; 64];
            let _ = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    match ctl_rx.read(&mut drain).await {
                        Ok(None) | Err(_) => break,
                        Ok(Some(_)) => continue,
                    }
                }
            })
            .await;
            Ok(SendReport { manifest, receipt })
        }
        Envelope {
            message: Message::Error { message },
            ..
        } => Err(TransferError::RemoteError(message)),
        Envelope { message, .. } => Err(TransferError::UnexpectedMessage(message)),
    }
}

/// Receive one transfer session on `transport`, staging under
/// `opts.staging_root` and finalizing into `opts.target_dir` only after
/// every file verifies. Returns the receipt it sent back.
pub async fn receive<T: Transport>(
    transport: &T,
    opts: &ReceiveOptions,
) -> Result<Receipt, TransferError> {
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

    // A fresh session reports an empty have-list; ticket 07 reconciles
    // against persisted state here instead.
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Accept {
            manifest_hash: manifest.hash.clone(),
            have: Vec::new(),
        }),
    )
    .await?;

    let stage_dir = staging_dir(&opts.staging_root, &manifest);
    let mut received: Vec<HashSet<u32>> = manifest.files.iter().map(|_| HashSet::new()).collect();

    let (_data_tx, mut data_rx) = transport.accept_bi().await?;
    loop {
        let (file, chunk, payload) = match read_data_frame(&mut data_rx).await {
            Ok(Some(frame)) => frame,
            Ok(None) => break, // clean FIN: sender finished the data stream
            Err(ProtocolError::Io(e))
                if matches!(
                    e.kind(),
                    ErrorKind::UnexpectedEof
                        | ErrorKind::ConnectionReset
                        | ErrorKind::ConnectionAborted
                        | ErrorKind::BrokenPipe
                ) =>
            {
                return Err(TransferError::ConnectionLost(e.to_string()));
            }
            Err(e) => return Err(e.into()),
        };

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
        let got = blake3::hash(&payload).to_hex().to_string();
        if got != chunk_info.hash {
            let err = TransferError::ChunkHashMismatch {
                path: entry.path.clone(),
                chunk: chunk as usize,
                expected: chunk_info.hash.clone(),
                got,
            };
            let _ = write_message(
                &mut ctl_tx,
                &Envelope::new(Message::Error {
                    message: err.to_string(),
                }),
            )
            .await;
            return Err(err);
        }

        // Verify-then-write; duplicates are idempotent no-ops.
        if received[file as usize].insert(chunk) {
            let staged = stage_dir.join(&entry.path);
            if let Some(parent) = staged.parent() {
                fs::create_dir_all(parent)?;
            }
            // Offset-based chunk writes: never truncate — resume (ticket 07)
            // relies on previously staged bytes surviving.
            let mut out = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&staged)?;
            out.seek(SeekFrom::Start(chunk_info.offset))?;
            out.write_all(&payload)?;
        }
    }

    match read_message(&mut ctl_rx).await? {
        Envelope {
            message: Message::Complete { manifest_hash },
            ..
        } => {
            if manifest_hash != manifest.hash {
                return Err(TransferError::WrongManifest(manifest_hash));
            }
        }
        Envelope {
            message: Message::Error { message },
            ..
        } => return Err(TransferError::RemoteError(message)),
        Envelope {
            message: Message::Cancel { reason },
            ..
        } => {
            // Cancellation keeps staging (resumable) and the target clean.
            return Err(TransferError::Cancelled(reason));
        }
        Envelope { message, .. } => return Err(TransferError::UnexpectedMessage(message)),
    }

    let receipt = verify_and_finalize(&manifest, &stage_dir, &opts.target_dir)?;

    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Receipt {
            receipt: receipt.clone(),
        }),
    )
    .await?;
    // Quiesce: FIN our control side and wait for the sender's (bounded), so
    // the Receipt is delivered before this side drops the connection — an
    // implicit close discards queued-but-unacked stream data.
    let _ = ctl_tx.finish();
    let mut drain = [0u8; 64];
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match ctl_rx.read(&mut drain).await {
                Ok(None) | Err(_) => break,
                Ok(Some(_)) => continue,
            }
        }
    })
    .await;
    Ok(receipt)
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

/// Verify every staged file against the manifest, and only then finalize
/// the whole tree into the target (collision-safe, never overwriting).
/// Builds the receipt from the verified files.
fn verify_and_finalize(
    manifest: &Manifest,
    stage_dir: &Path,
    target_dir: &Path,
) -> Result<Receipt, TransferError> {
    let mut files = Vec::with_capacity(manifest.files.len());
    for entry in &manifest.files {
        let staged = stage_dir.join(&entry.path);
        if entry.size == 0 && !staged.exists() {
            // Empty files sent no frames; materialize them now.
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
        let got = hash_reader(fs::File::open(&staged)?)?;
        if got != entry.hash {
            return Err(TransferError::FileHashMismatch {
                path: entry.path.clone(),
                expected: entry.hash.clone(),
                got,
            });
        }
        files.push(ReceiptFile {
            path: entry.path.clone(),
            size: entry.size,
            hash: entry.hash.clone(),
            chunks: entry.chunks.len() as u64,
        });
    }

    // Everything verified — this is the only code path that writes into the
    // target directory.
    fs::create_dir_all(target_dir)?;
    for dir in &manifest.empty_dirs {
        fs::create_dir_all(target_dir.join(dir))?;
    }
    for entry in &manifest.files {
        let staged = stage_dir.join(&entry.path);
        let dest = collision_free_path(&target_dir.join(&entry.path))?;
        if let Some(parent) = dest.parent() {
            // Parent dirs beyond the target root (e.g. `bundle/docs/`) are
            // not in empty_dirs — they exist only implicitly, via their files.
            fs::create_dir_all(parent)?;
        }
        move_into_place(&staged, &dest)?;
    }
    let _ = fs::remove_dir_all(stage_dir);

    Ok(Receipt {
        manifest_hash: manifest.hash.clone(),
        files,
    })
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
    let mut stack: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    while let Some(path) = stack.pop() {
        if path.is_dir() {
            if let Ok(entries) = fs::read_dir(&path) {
                stack.extend(entries.filter_map(|e| e.ok()).map(|e| e.path()));
            }
        } else if let Ok(md) = path.metadata() {
            total += md.len();
        }
    }
    total
}
