//! Ticket 05 acceptance tests: protocol framing, versioned envelope,
//! unknown-type rejection, data frames, and receipt verification.

use std::io::Cursor;

use dasho_core::{
    chunk::{ChunkInfo, ChunkParams},
    manifest::{FileEntry, Manifest, Receipt, ReceiptFile, build_manifest},
    protocol::{
        ChunkId, Envelope, HaveFile, Message, PROTOCOL_VERSION, read_data_frame, read_message,
        write_data_frame, write_message,
    },
};
use tempfile::TempDir;
use tokio::io::duplex;

fn sample_manifest() -> Manifest {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("note.txt"), b"protocol test").unwrap();
    build_manifest(&[dir.path().join("note.txt")], &ChunkParams::default()).unwrap()
}

fn sample_receipt(manifest: &Manifest) -> Receipt {
    Receipt {
        manifest_hash: manifest.hash.clone(),
        files: manifest
            .files
            .iter()
            .map(|f| ReceiptFile {
                path: f.path.clone(),
                size: f.size,
                hash: f.hash.clone(),
                chunks: f.chunks.len() as u64,
            })
            .collect(),
    }
}

#[tokio::test]
async fn every_message_kind_round_trips() {
    let manifest = sample_manifest();
    let receipt = sample_receipt(&manifest);
    let messages = vec![
        Message::Hello {
            name: "laptop".into(),
            fingerprint: "AAAA-BBBB".into(),
        },
        Message::Offer {
            manifest: manifest.clone(),
        },
        Message::Accept {
            manifest_hash: manifest.hash.clone(),
            have: vec![HaveFile {
                file: 0,
                chunks: vec![0, 3, 7],
            }],
            streams: 4,
        },
        Message::Complete {
            manifest_hash: manifest.hash.clone(),
        },
        Message::Missing {
            manifest_hash: manifest.hash.clone(),
            chunks: vec![ChunkId { file: 1, chunk: 2 }, ChunkId { file: 4, chunk: 0 }],
        },
        Message::Receipt {
            receipt: receipt.clone(),
        },
        Message::Cancel {
            reason: "user asked".into(),
        },
        Message::Error {
            message: "disk full".into(),
        },
    ];
    for msg in messages {
        let (mut client, mut server) = duplex(64 * 1024);
        write_message(&mut client, &Envelope::new(msg.clone()))
            .await
            .unwrap();
        let back = read_message(&mut server).await.unwrap();
        assert_eq!(back.version, PROTOCOL_VERSION);
        assert_eq!(back.message, msg);
    }
}

#[tokio::test]
async fn unknown_message_type_is_rejected_cleanly() {
    // Hand-crafted wire bytes: version 1, message with a bogus type tag.
    let payload = br#"{"v":1,"m":{"type":"banana","ripe":true}}"#;
    let mut wire = Vec::new();
    wire.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    wire.extend_from_slice(payload);
    let mut reader = Cursor::new(wire);
    let err = read_message(&mut reader).await.unwrap_err();
    assert!(
        err.to_string().contains("unknown message type"),
        "typed rejection expected, got: {err}"
    );
}

#[tokio::test]
async fn future_protocol_version_is_rejected() {
    let payload = br#"{"v":99,"m":{"type":"hello","name":"x","fingerprint":"y"}}"#;
    let mut wire = Vec::new();
    wire.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    wire.extend_from_slice(payload);
    let mut reader = Cursor::new(wire);
    let err = read_message(&mut reader).await.unwrap_err();
    assert!(err.to_string().contains("version"), "got: {err}");
}

#[tokio::test]
async fn oversized_length_prefix_is_rejected() {
    use tokio::io::AsyncWriteExt;
    let (mut client, mut server) = duplex(64);
    client.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
    let err = read_message(&mut server).await.unwrap_err();
    assert!(err.to_string().contains("exceeds"), "got: {err}");
}

#[tokio::test]
async fn truncated_message_is_a_clean_error() {
    let payload = br#"{"v":1,"m":{"type":"hello""#; // cut mid-JSON
    let mut wire = Vec::new();
    wire.extend_from_slice(&(payload.len() as u32 + 100).to_be_bytes()); // length lies
    wire.extend_from_slice(payload);
    let mut reader = Cursor::new(wire);
    let result = read_message(&mut reader).await;
    assert!(result.is_err(), "truncated message must not parse");
}

#[tokio::test]
async fn data_frames_round_trip() {
    let (mut client, mut server) = duplex(1024 * 1024);
    let payload: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    write_data_frame(&mut client, 3, 11, &payload)
        .await
        .unwrap();
    write_data_frame(&mut client, 4, 0, b"tiny").await.unwrap();
    let (f1, c1, p1) = read_data_frame(&mut server)
        .await
        .unwrap()
        .expect("frame 1");
    assert_eq!((f1, c1), (3, 11));
    assert_eq!(p1, payload);
    let (f2, c2, p2) = read_data_frame(&mut server)
        .await
        .unwrap()
        .expect("frame 2");
    assert_eq!((f2, c2, p2), (4, 0, b"tiny".to_vec()));
}

#[tokio::test]
async fn clean_eof_after_frames_returns_none() {
    let (mut client, mut server) = duplex(1024);
    write_data_frame(&mut client, 0, 0, b"x").await.unwrap();
    // Dropping the sending half closes the stream cleanly.
    drop(client);
    assert!(read_data_frame(&mut server).await.unwrap().is_some());
    assert!(
        read_data_frame(&mut server).await.unwrap().is_none(),
        "end of stream must be a clean Ok(None), not an error"
    );
}

#[tokio::test]
async fn data_frame_with_wrong_version_is_rejected() {
    use tokio::io::AsyncWriteExt;
    let (mut client, mut server) = duplex(1024);
    let mut frame = vec![9u8]; // wrong frame version
    frame.extend_from_slice(&0u32.to_be_bytes());
    frame.extend_from_slice(&0u32.to_be_bytes());
    frame.extend_from_slice(&4u64.to_be_bytes());
    frame.extend_from_slice(b"body");
    client.write_all(&frame).await.unwrap();
    let err = read_data_frame(&mut server).await.unwrap_err();
    assert!(err.to_string().contains("frame version"), "got: {err}");
}

#[tokio::test]
async fn truncated_data_frame_is_an_error_not_none() {
    use tokio::io::AsyncWriteExt;
    let (mut client, mut server) = duplex(1024);
    // Correct 17-byte header claiming 64 bytes, but only 10 bytes follow.
    let mut frame = vec![1u8];
    frame.extend_from_slice(&0u32.to_be_bytes());
    frame.extend_from_slice(&0u32.to_be_bytes());
    frame.extend_from_slice(&64u64.to_be_bytes());
    frame.extend_from_slice(b"0123456789");
    client.write_all(&frame).await.unwrap();
    drop(client);
    assert!(
        read_data_frame(&mut server).await.is_err(),
        "a frame cut mid-payload must error, not report end-of-stream"
    );
}

#[tokio::test]
async fn manifest_serializes_inside_offer_with_chunks() {
    // Guard the JSON shape of an Offer carrying a multi-chunk file. 300 KiB
    // exceeds max_size (256 KiB), so >= 2 chunks is guaranteed, not probabilistic.
    let dir = TempDir::new().unwrap();
    let blob: Vec<u8> = (0..300 * 1024u32).map(|i| (i % 253) as u8).collect();
    std::fs::write(dir.path().join("blob.bin"), &blob).unwrap();
    let m = build_manifest(&[dir.path().join("blob.bin")], &ChunkParams::default()).unwrap();
    assert!(m.files[0].chunks.len() >= 2);
    let env = Envelope::new(Message::Offer {
        manifest: m.clone(),
    });
    let (mut a, mut b) = duplex(1024 * 1024);
    write_message(&mut a, &env).await.unwrap();
    let back = read_message(&mut b).await.unwrap();
    match back.message {
        Message::Offer { manifest } => {
            assert_eq!(manifest, m);
            assert!(manifest.verify_hash());
        }
        other => panic!("expected Offer, got {other:?}"),
    }
}

#[test]
fn receipt_verifies_against_manifest_and_detects_tampering() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), b"receipt test").unwrap();
    let m = build_manifest(&[dir.path().join("a.txt")], &ChunkParams::default()).unwrap();
    let receipt = sample_receipt(&m);
    assert!(receipt.verify_against(&m), "honest receipt must verify");

    let mut tampered = receipt.clone();
    tampered.files[0].hash = "0".repeat(64);
    assert!(!tampered.verify_against(&m), "wrong hash must fail");

    let mut short = receipt.clone();
    short.files[0].chunks += 1;
    assert!(!short.verify_against(&m), "wrong chunk count must fail");

    let mut renamed = receipt.clone();
    renamed.files[0].path = "other.txt".into();
    assert!(!renamed.verify_against(&m), "wrong path must fail");

    let empty = Manifest {
        files: vec![FileEntry {
            path: "x".into(),
            size: 1,
            hash: "h".into(),
            chunks: vec![ChunkInfo {
                offset: 0,
                length: 1,
                hash: "c".into(),
            }],
        }],
        empty_dirs: vec![],
        total_bytes: 1,
        hash: "other".into(),
    };
    assert!(
        !receipt.verify_against(&empty),
        "different manifest must fail"
    );
}
