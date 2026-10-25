//! Content-defined chunking (FastCDC 2020) with BLAKE3 per-chunk and
//! whole-file hashing.
//!
//! Chunk parameters live in ONE place ([`ChunkParams::default`]) so sender and
//! receiver can never disagree: the manifest records per-chunk hashes, and the
//! receiver re-hashes with the same BLAKE3, but cut points are produced by the
//! sender only.

use std::io::Read;

use fastcdc::v2020::StreamCDC;

/// The BLAKE3 hash of the empty input (useful constant for empty files).
pub const EMPTY_BLAKE3: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";

/// Content-defined chunking parameters.
///
/// Rationale for the defaults (16 KiB min / 64 KiB avg / 256 KiB max):
/// Dasho's workload is large media on fast LANs (Wi-Fi 6 / Ethernet), where
/// 64 KiB average chunks amortize QUIC stream-write overhead while keeping
/// dedup and resume granularity fine enough that a lost connection costs at
/// most a few hundred KiB of re-transfer. The 16 KiB floor avoids tiny chunks
/// whose per-frame overhead dominates on loopback, and the 256 KiB ceiling
/// bounds receiver staging memory and the cost of re-requesting one chunk.
/// All values must be even (FastCDC scans byte pairs) and within the crate's
/// hard limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkParams {
    /// Minimum chunk size in bytes.
    pub min_size: usize,
    /// Average (target) chunk size in bytes.
    pub avg_size: usize,
    /// Maximum chunk size in bytes.
    pub max_size: usize,
}

impl Default for ChunkParams {
    fn default() -> Self {
        Self {
            min_size: 16 * 1024,
            avg_size: 64 * 1024,
            max_size: 256 * 1024,
        }
    }
}

impl ChunkParams {
    /// Validate against the FastCDC implementation's hard constraints
    /// (checked there only via `debug_assert`, so we enforce them ourselves).
    pub fn validate(&self) -> Result<(), ChunkError> {
        let invalid = |msg: &str| Err(ChunkError::InvalidParams(msg.to_string()));
        if self.min_size < 64 || self.min_size % 2 != 0 {
            return invalid("min_size must be >= 64 and even");
        }
        if self.avg_size < 256 || self.avg_size % 2 != 0 {
            return invalid("avg_size must be >= 256 and even");
        }
        if self.max_size < 1024 || self.max_size % 2 != 0 {
            return invalid("max_size must be >= 1024 and even");
        }
        if self.min_size >= self.avg_size || self.avg_size >= self.max_size {
            return invalid("must have min_size < avg_size < max_size");
        }
        Ok(())
    }
}

/// One content-addressed chunk: where it starts, how long it is, and its
/// BLAKE3 hash (hex).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChunkInfo {
    pub offset: u64,
    pub length: u64,
    pub hash: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ChunkError {
    #[error("I/O error while chunking or hashing: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid chunk parameters: {0}")]
    InvalidParams(String),
    #[error("chunker failure: {0}")]
    Chunker(String),
}

/// Chunk everything readable from `reader`, returning chunk metadata in
/// stream order. Empty input yields an empty vector.
pub fn chunk_reader(reader: impl Read, params: &ChunkParams) -> Result<Vec<ChunkInfo>, ChunkError> {
    params.validate()?;
    let chunker = StreamCDC::new(reader, params.min_size, params.avg_size, params.max_size);
    let mut chunks = Vec::new();
    for entry in chunker {
        let entry = entry.map_err(|e| ChunkError::Chunker(e.to_string()))?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(&entry.data);
        chunks.push(ChunkInfo {
            offset: entry.offset,
            length: entry.length as u64,
            hash: hasher.finalize().to_hex().to_string(),
        });
    }
    Ok(chunks)
}

/// Whole-file BLAKE3 hash (hex) of everything readable from `reader`.
pub fn hash_reader(reader: impl Read) -> Result<String, ChunkError> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(reader)?;
    Ok(hasher.finalize().to_hex().to_string())
}
