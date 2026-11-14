//! Ticket 05 acceptance tests: content-defined chunking with BLAKE3 hashing.

use std::io::Cursor;

use todoke_core::chunk::{ChunkParams, chunk_reader, hash_reader};

/// Deterministic pseudo-random bytes (splitmix64) so tests never depend on
/// entropy; a plain LCG's top byte is too structured for content-defined
/// chunking tests (short periods can dodge cut points entirely).
fn pseudo_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed;
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        out.extend_from_slice(&z.to_le_bytes());
    }
    out.truncate(len);
    out
}

#[test]
fn chunking_is_deterministic() {
    let data = pseudo_bytes(42, 1024 * 1024);
    let a = chunk_reader(Cursor::new(&data), &ChunkParams::default()).unwrap();
    let b = chunk_reader(Cursor::new(&data), &ChunkParams::default()).unwrap();
    assert!(!a.is_empty(), "1 MiB must yield chunks");
    assert_eq!(a, b, "same input must always produce the same chunk list");
}

#[test]
fn chunks_cover_input_exactly() {
    let params = ChunkParams::default();
    let data = pseudo_bytes(7, 300 * 1024);
    let chunks = chunk_reader(Cursor::new(&data), &params).unwrap();
    let mut next_offset = 0u64;
    for c in &chunks {
        assert_eq!(c.offset, next_offset, "chunks must be contiguous");
        assert!(c.length > 0, "chunks must be non-empty");
        assert!(
            c.length as usize <= params.max_size,
            "chunk length {} exceeds max {}",
            c.length,
            params.max_size
        );
        next_offset = c.offset + c.length;
    }
    assert_eq!(
        next_offset,
        data.len() as u64,
        "every byte chunked exactly once"
    );
}

#[test]
fn empty_input_yields_no_chunks() {
    let chunks = chunk_reader(Cursor::new(&[]), &ChunkParams::default()).unwrap();
    assert!(chunks.is_empty(), "empty file has zero chunks");
}

#[test]
fn empty_input_hashes_to_known_blake3_constant() {
    // Known-good BLAKE3 of the empty input, independent of our code path.
    const EMPTY: &str = "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262";
    let h = hash_reader(Cursor::new(&[])).unwrap();
    assert_eq!(h, EMPTY);
}

#[test]
fn sub_minimum_input_is_one_chunk() {
    let params = ChunkParams::default();
    let data = pseudo_bytes(1, 10_000); // < min_size
    let chunks = chunk_reader(Cursor::new(&data), &params).unwrap();
    assert_eq!(chunks.len(), 1, "input below min size is a single chunk");
    assert_eq!(chunks[0].offset, 0);
    assert_eq!(chunks[0].length, 10_000);
    // The chunk hash must equal the whole-file hash of the same bytes.
    let whole = hash_reader(Cursor::new(&data)).unwrap();
    assert_eq!(chunks[0].hash, whole);
}

#[test]
fn default_params_are_pinned_and_valid() {
    let p = ChunkParams::default();
    // The single source of truth for chunk sizing (documented in chunk.rs).
    assert_eq!(p.min_size, 16 * 1024);
    assert_eq!(p.avg_size, 64 * 1024);
    assert_eq!(p.max_size, 256 * 1024);
    p.validate().unwrap();
}

#[test]
fn odd_or_out_of_range_params_are_rejected() {
    let bad = [
        ChunkParams {
            min_size: 16 * 1024 + 1,
            avg_size: 64 * 1024,
            max_size: 256 * 1024,
        }, // odd
        ChunkParams {
            min_size: 32,
            avg_size: 64 * 1024,
            max_size: 256 * 1024,
        }, // min < 64
        ChunkParams {
            min_size: 16 * 1024,
            avg_size: 256 * 1024 + 2,
            max_size: 256 * 1024,
        }, // avg > max
    ];
    for p in bad {
        assert!(p.validate().is_err(), "params {p:?} must be rejected");
        assert!(chunk_reader(Cursor::new(&pseudo_bytes(3, 100)), &p).is_err());
    }
}
