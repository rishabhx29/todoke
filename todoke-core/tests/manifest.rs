//! Ticket 05 acceptance tests: content-addressed manifests.

use std::{fs, path::PathBuf};

use tempfile::TempDir;
use todoke_core::{
    chunk::{ChunkParams, EMPTY_BLAKE3},
    manifest::build_manifest,
};

fn write(path: &PathBuf, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, bytes).unwrap();
}

/// Deterministic pseudo-random bytes (splitmix64) — content-defined chunking
/// only cuts early on varied data; a plain LCG's top byte is too structured.
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

fn tree() -> TempDir {
    let dir = TempDir::new().unwrap();
    write(&dir.path().join("docs/readme.txt"), b"hello todoke");
    write(&dir.path().join("docs/empty.bin"), b"");
    write(
        &dir.path().join("media/video.dat"),
        &pseudo_bytes(9, 200 * 1024),
    );
    dir
}

#[test]
fn same_tree_produces_same_manifest_hash() {
    let a = build_manifest(&[tree().path().join("docs")], &ChunkParams::default()).unwrap();
    let b = build_manifest(&[tree().path().join("docs")], &ChunkParams::default()).unwrap();
    assert_eq!(a.hash, b.hash, "identical content must hash identically");
    assert!(a.verify_hash(), "manifest must verify against its own hash");
}

#[test]
fn hash_is_content_addressed_not_location_addressed() {
    let first = tree();
    let a = build_manifest(&[first.path().join("docs")], &ChunkParams::default()).unwrap();
    // Same tree (same name, same content), different absolute location.
    let second = TempDir::new().unwrap();
    write(&second.path().join("docs/readme.txt"), b"hello todoke");
    write(&second.path().join("docs/empty.bin"), b"");
    let b = build_manifest(&[second.path().join("docs")], &ChunkParams::default()).unwrap();
    assert_eq!(a.hash, b.hash, "location must not affect the manifest hash");
}

#[test]
fn changed_content_changes_hash() {
    let a = build_manifest(&[tree().path().join("docs")], &ChunkParams::default()).unwrap();
    let changed = TempDir::new().unwrap();
    write(&changed.path().join("readme.txt"), b"hello todoke!");
    write(&changed.path().join("empty.bin"), b"");
    let b = build_manifest(&[changed.path()], &ChunkParams::default()).unwrap();
    assert_ne!(
        a.hash, b.hash,
        "content change must change the manifest hash"
    );
}

#[test]
fn input_order_does_not_change_hash() {
    let dir = tree();
    let media = dir.path().join("media");
    let docs = dir.path().join("docs");
    let a = build_manifest(&[docs.clone(), media.clone()], &ChunkParams::default()).unwrap();
    let b = build_manifest(&[media, docs], &ChunkParams::default()).unwrap();
    assert_eq!(
        a.hash, b.hash,
        "files are sorted internally; input order is irrelevant"
    );
}

#[test]
fn files_are_sorted_with_portable_relative_paths() {
    let dir = tree();
    let m = build_manifest(&[dir.path().join("docs")], &ChunkParams::default()).unwrap();
    let paths: Vec<&str> = m.files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, vec!["docs/empty.bin", "docs/readme.txt"]);
    assert!(m.total_bytes > 0);
}

#[test]
fn single_file_input_lands_under_its_own_name() {
    let dir = tree();
    let m = build_manifest(
        &[dir.path().join("docs/readme.txt")],
        &ChunkParams::default(),
    )
    .unwrap();
    assert_eq!(m.files.len(), 1);
    assert_eq!(m.files[0].path, "readme.txt");
    assert_eq!(m.files[0].size, 12);
}

#[test]
fn empty_file_entry_has_zero_chunks_and_known_hash() {
    let dir = tree();
    let m = build_manifest(&[dir.path().join("docs")], &ChunkParams::default()).unwrap();
    let empty = m.files.iter().find(|f| f.path == "docs/empty.bin").unwrap();
    assert_eq!(empty.size, 0);
    assert!(empty.chunks.is_empty());
    assert_eq!(empty.hash, EMPTY_BLAKE3);
}

#[test]
fn large_file_is_split_into_multiple_chunks() {
    let dir = tree();
    let m = build_manifest(&[dir.path().join("media")], &ChunkParams::default()).unwrap();
    let video = m
        .files
        .iter()
        .find(|f| f.path.ends_with("video.dat"))
        .unwrap();
    assert!(
        video.chunks.len() >= 2,
        "200 KiB must produce multiple chunks"
    );
    let chunk_total: u64 = video.chunks.iter().map(|c| c.length).sum();
    assert_eq!(
        chunk_total, video.size,
        "chunks must cover the file exactly"
    );
}

#[test]
fn tampered_manifest_fails_hash_verification() {
    let mut m = build_manifest(&[tree().path().join("docs")], &ChunkParams::default()).unwrap();
    m.files[0].size += 1;
    assert!(!m.verify_hash(), "tampering must be detectable");
}

#[test]
fn empty_directories_are_recorded() {
    let dir = TempDir::new().unwrap();
    fs::create_dir_all(dir.path().join("photos/2024/empty")).unwrap();
    write(&dir.path().join("photos/keep.txt"), b"x");
    let m = build_manifest(&[dir.path().join("photos")], &ChunkParams::default()).unwrap();
    assert_eq!(m.empty_dirs, vec!["photos/2024/empty".to_string()]);
}

#[test]
fn nonexistent_input_is_a_typed_error() {
    let err = build_manifest(
        &[PathBuf::from("/definitely/not/here")],
        &ChunkParams::default(),
    )
    .unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("not found"),
        "got: {err}"
    );
}
