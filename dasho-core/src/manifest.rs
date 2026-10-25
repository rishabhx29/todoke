//! Content-addressed transfer manifests.
//!
//! A manifest describes everything a session will send: sorted relative file
//! paths with sizes, whole-file BLAKE3 hashes, and per-chunk BLAKE3 hashes
//! from content-defined chunking. The manifest hash is the BLAKE3 of the
//! canonical JSON serialization of that list — the same tree always produces
//! the same manifest hash, which is what receivers key their staging state on
//! (resume, receipts) regardless of where the tree lives on the sender.
//!
//! Path semantics (mirrors `scp`/`rsync` intuition): a single file input is
//! offered under its own name; a directory input is offered under the
//! directory's name including its whole subtree.

use std::{
    fs::File,
    io::{BufReader, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::chunk::{ChunkInfo, ChunkParams, chunk_reader, hash_reader};

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("I/O error while building manifest: {0}")]
    Io(#[from] std::io::Error),
    #[error("chunking error: {0}")]
    Chunk(#[from] crate::chunk::ChunkError),
    #[error("input path not found: {0}")]
    NotFound(String),
    #[error("duplicate relative path in manifest inputs: {0}")]
    DuplicatePath(String),
}

/// One file in a manifest, with its chunk list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative path, '/'-separated, stable across operating systems.
    pub path: String,
    pub size: u64,
    /// Whole-file BLAKE3 (hex).
    pub hash: String,
    /// Chunks in stream order; empty for empty files.
    pub chunks: Vec<ChunkInfo>,
}

/// A full transfer manifest: the offer the sender makes and the receiver
/// verifies, hashes, and reconciles against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Files sorted by relative path (canonical order).
    pub files: Vec<FileEntry>,
    /// Directories that contain no files anywhere below them but must be
    /// recreated (deepest empty dirs; parents are implied by create_dir_all).
    pub empty_dirs: Vec<String>,
    pub total_bytes: u64,
    /// BLAKE3 (hex) of the canonical serialization of this manifest.
    pub hash: String,
}

/// The canonical, hash-input form of a manifest (no precomputed hash field;
/// field order is fixed, files pre-sorted, so serialization is deterministic).
#[derive(Serialize)]
struct CanonicalManifest<'a> {
    files: &'a [FileEntry],
    empty_dirs: &'a [String],
    total_bytes: u64,
}

fn canonical_hash(files: &[FileEntry], empty_dirs: &[String], total_bytes: u64) -> String {
    let canonical = CanonicalManifest {
        files,
        empty_dirs,
        total_bytes,
    };
    // serde_json serialization of structs is deterministic; unwrap is safe
    // (no non-string keys, no unserializable types) but guarded anyway.
    let bytes = serde_json::to_vec(&canonical).unwrap_or_default();
    blake3::hash(&bytes).to_hex().to_string()
}

impl Manifest {
    /// Recompute the content hash of this manifest's contents.
    pub fn compute_hash(&self) -> String {
        canonical_hash(&self.files, &self.empty_dirs, self.total_bytes)
    }

    /// True when the stored hash matches the manifest's actual content.
    pub fn verify_hash(&self) -> bool {
        self.compute_hash() == self.hash
    }

    /// Total number of chunks across all files.
    pub fn total_chunks(&self) -> usize {
        self.files.iter().map(|f| f.chunks.len()).sum()
    }
}

/// Per-file proof-of-receipt entry: what the receiver claims to have
/// finalized, verified by the sender against its manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceiptFile {
    pub path: String,
    pub size: u64,
    /// Whole-file BLAKE3 (hex) as computed by the receiver.
    pub hash: String,
    /// Number of chunks the file was reassembled from.
    pub chunks: u64,
}

/// Verification receipt returned by the receiver at the end of a transfer
/// and written to disk in the transfer record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub manifest_hash: String,
    pub files: Vec<ReceiptFile>,
}

impl Receipt {
    /// Sender-side verification: does this receipt exactly match the offered
    /// manifest (same files, sizes, whole-file hashes, chunk counts)?
    pub fn verify_against(&self, manifest: &Manifest) -> bool {
        if self.manifest_hash != manifest.hash || self.files.len() != manifest.files.len() {
            return false;
        }
        self.files.iter().zip(&manifest.files).all(|(r, m)| {
            r.path == m.path
                && r.size == m.size
                && r.hash == m.hash
                && r.chunks == m.chunks.len() as u64
        })
    }

    /// Sum of received file sizes.
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// Collect the files under `root` as `(absolute, relative-to-root)` pairs plus
/// the deepest empty directories. `root` may itself be a file.
type Walked = (Vec<(PathBuf, String)>, Vec<String>);

fn walk(root: &Path) -> Result<Walked, ManifestError> {
    fn walk_dir(
        dir: &Path,
        prefix: &str,
        files: &mut Vec<(PathBuf, String)>,
        empty_dirs: &mut Vec<String>,
    ) -> Result<(), ManifestError> {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .collect();
        if entries.is_empty() {
            empty_dirs.push(prefix.to_string());
            return Ok(());
        }
        entries.sort();
        for entry in entries {
            let name = entry
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let rel = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if entry.is_dir() {
                walk_dir(&entry, &rel, files, empty_dirs)?;
            } else if entry.is_file() {
                files.push((entry, rel));
            }
        }
        Ok(())
    }

    let mut files = Vec::new();
    let mut empty_dirs = Vec::new();
    if root.is_file() {
        let name = root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        files.push((root.to_path_buf(), name));
    } else if root.is_dir() {
        walk_dir(root, "", &mut files, &mut empty_dirs)?;
    } else {
        return Err(ManifestError::NotFound(root.display().to_string()));
    }
    Ok((files, empty_dirs))
}

/// A manifest plus the absolute source path of every file in it, aligned by
/// index: everything a sender needs to actually read the bytes it offered.
#[derive(Debug, Clone)]
pub struct TransferUnit {
    pub manifest: Manifest,
    pub sources: Vec<PathBuf>,
}

/// Build a manifest for the given files/directories.
pub fn build_manifest(
    inputs: &[impl AsRef<Path>],
    params: &ChunkParams,
) -> Result<Manifest, ManifestError> {
    let (manifest, _sources) = build_manifest_and_sources(inputs, params)?;
    Ok(manifest)
}

/// Build a manifest and remember where each file's bytes live on disk.
pub fn build_manifest_and_paths(
    inputs: &[impl AsRef<Path>],
    params: &ChunkParams,
) -> Result<TransferUnit, ManifestError> {
    let (manifest, sources) = build_manifest_and_sources(inputs, params)?;
    Ok(TransferUnit { manifest, sources })
}

fn build_manifest_and_sources(
    inputs: &[impl AsRef<Path>],
    params: &ChunkParams,
) -> Result<(Manifest, Vec<PathBuf>), ManifestError> {
    let mut sorted: Vec<PathBuf> = inputs.iter().map(|p| p.as_ref().to_path_buf()).collect();
    if sorted.is_empty() {
        return Err(ManifestError::NotFound("(no inputs)".to_string()));
    }
    for p in &sorted {
        if !p.exists() {
            return Err(ManifestError::NotFound(p.display().to_string()));
        }
    }
    sorted.sort();
    sorted.dedup();

    let mut files: Vec<(PathBuf, String)> = Vec::new();
    let mut empty_dirs: Vec<String> = Vec::new();
    for input in &sorted {
        let (walked, empties) = walk(input)?;
        let is_file = input.is_file();
        let input_name = input
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        for (abs, rel_within) in walked {
            let rel = if is_file {
                rel_within
            } else {
                format!("{input_name}/{rel_within}")
            };
            files.push((abs, rel));
        }
        for e in empties {
            empty_dirs.push(format!("{input_name}/{e}"));
        }
    }

    files.sort_by(|a, b| a.1.cmp(&b.1));
    if let Some(dup) = files
        .windows(2)
        .find(|w| w[0].1 == w[1].1)
        .map(|w| w[1].1.clone())
    {
        return Err(ManifestError::DuplicatePath(dup));
    }
    empty_dirs.sort();
    empty_dirs.dedup();

    let mut entries = Vec::with_capacity(files.len());
    let mut sources = Vec::with_capacity(files.len());
    let mut total_bytes = 0u64;
    for (abs, rel) in files {
        let file = File::open(&abs)?;
        let mut reader = BufReader::new(file);
        let size = reader.get_ref().metadata()?.len();
        let hash = hash_reader(&mut reader)?;
        reader.seek(SeekFrom::Start(0))?;
        let chunks = chunk_reader(reader, params)?;
        total_bytes += size;
        sources.push(abs);
        entries.push(FileEntry {
            path: rel,
            size,
            hash,
            chunks,
        });
    }

    let hash = canonical_hash(&entries, &empty_dirs, total_bytes);
    Ok((
        Manifest {
            files: entries,
            empty_dirs,
            total_bytes,
            hash,
        },
        sources,
    ))
}
