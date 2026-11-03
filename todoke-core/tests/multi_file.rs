//! Ticket 06 acceptance tests: multi-file transfers with parallel streams,
//! small-file packing, progress events, cancel semantics, collisions, and
//! per-file failure identification.

use std::{fs, path::Path, path::PathBuf, time::Duration};

use rustls::pki_types::CertificateDer;
use tempfile::TempDir;
use todoke_core::{
    chunk::{ChunkParams, hash_reader},
    identity::DeviceIdentity,
    manifest::build_manifest,
    protocol::{Envelope, Message, read_message, write_data_frame, write_message},
    transfer::{self, ReceiveOptions, SendOptions, SenderInfo, TransferError, TransferEvent},
    transport,
};
use tokio::sync::mpsc;

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

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("todoke-multi-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct Peer {
    fingerprint: String,
    addr: std::net::SocketAddr,
    cert: CertificateDer<'static>,
}

/// Spawn a one-session receiver; returns the peer and the receiver's result.
async fn spawn_receiver(
    tag: &str,
    opts: ReceiveOptions,
) -> (
    Peer,
    tokio::task::JoinHandle<Result<todoke_core::manifest::Receipt, TransferError>>,
) {
    let id = DeviceIdentity::load_or_create(&tmpdir(tag)).unwrap();
    let server_id = transport::server_identity(&id).unwrap();
    let fingerprint = id.fingerprint();
    let cert = CertificateDer::from(server_id.cert_der().to_vec());
    let (endpoint, addr) = transport::serve(&server_id, "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let handle = tokio::spawn(async move {
        let incoming = endpoint.accept().await.expect("incoming");
        let conn = incoming.await.expect("handshake");
        let (endpoint, conn) = (endpoint, conn);
        let result = transfer::receive(&conn, &opts).await;
        drop((endpoint, conn));
        result
    });
    (
        Peer {
            fingerprint,
            addr,
            cert,
        },
        handle,
    )
}

async fn connect(peer: &Peer) -> (quinn::Endpoint, quinn::Connection) {
    transport::dial(peer.addr, &peer.fingerprint, peer.cert.clone())
        .await
        .expect("connect")
}

fn sender_info(peer: &Peer) -> SenderInfo {
    SenderInfo {
        name: "multi-test".into(),
        fingerprint: peer.fingerprint.clone(),
    }
}

/// The acceptance fixture: 2000 small files across nested dirs (unicode +
/// long names), one empty dir, and three ~16 MB files.
fn build_fixture(root: &Path) {
    let mut long_name = String::from("l");
    while long_name.len() < 180 {
        long_name.push('o');
    }
    long_name.push_str(".dat");

    for i in 0..2000u32 {
        let dir = root
            .join(format!("batch{}", i / 500))
            .join(format!("sub{}", i % 7));
        fs::create_dir_all(&dir).unwrap();
        let name = if i == 42 {
            root.join(format!("greek/α-β-{i}测量.dat"))
        } else if i == 43 {
            root.join("greek").join(&long_name)
        } else {
            dir.join(format!("notes-{i}.txt"))
        };
        if let Some(parent) = name.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(
            &name,
            format!("contents of file {i} — {}", "x".repeat(i as usize % 40)),
        )
        .unwrap();
    }
    fs::create_dir_all(root.join("batch0/sub0/empty-folder")).unwrap();

    fs::create_dir_all(root.join("media")).unwrap();
    for (i, seed) in [7u64, 8, 9].iter().enumerate() {
        fs::write(
            root.join(format!("media/render-{i}.bin")),
            pseudo_bytes(*seed, 16 * 1024 * 1024),
        )
        .unwrap();
    }
}

#[tokio::test]
async fn big_tree_transfers_byte_identical_with_all_files() {
    let src = TempDir::new().unwrap();
    build_fixture(src.path());
    let manifest = build_manifest(&[src.path().to_path_buf()], &ChunkParams::default()).unwrap();
    assert_eq!(
        manifest.files.len(),
        2003,
        "expected 2003 files, got {}",
        manifest.files.len()
    );
    assert!(manifest.total_bytes > 48 * 1024 * 1024);

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver(
        "bigtree",
        ReceiveOptions::new(target.path().to_path_buf(), staging.path().to_path_buf()),
    )
    .await;
    let (_ep, conn) = connect(&peer).await;

    let started = std::time::Instant::now();
    let report = tokio::time::timeout(
        Duration::from_secs(240),
        transfer::send(&conn, &[src.path().to_path_buf()], &sender_info(&peer)),
    )
    .await
    .expect("send finishes")
    .expect("send succeeds");
    let elapsed = started.elapsed();
    tokio::time::timeout(Duration::from_secs(60), receiver)
        .await
        .expect("receiver finishes")
        .expect("receiver task alive")
        .expect("receive succeeds");
    println!("48MB / 2003 files over 4 streams: {elapsed:?}");

    // Byte-identical everywhere, incl. unicode + the 180-char name.
    for entry in &report.manifest.files {
        let got = target.path().join(&entry.path);
        assert!(got.is_file(), "missing {}", entry.path);
        assert_eq!(
            hash_reader(fs::File::open(&got).unwrap()).unwrap(),
            entry.hash,
            "{}",
            entry.path
        );
    }
    // Empty dir recreated.
    assert!(
        target.path().join(&manifest.empty_dirs[0]).is_dir(),
        "empty dir {} must be recreated",
        manifest.empty_dirs[0]
    );
    // Receipt covers everything.
    assert_eq!(report.receipt.files.len(), 2003);
    assert!(report.receipt.verify_against(&report.manifest));
}

#[tokio::test]
async fn parallel_streams_are_actually_concurrent() {
    let src = TempDir::new().unwrap();
    for i in 0..8u32 {
        fs::write(
            src.path().join(format!("m{i}.bin")),
            pseudo_bytes(i as u64 + 1, 4 * 1024 * 1024),
        )
        .unwrap();
    }
    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (stats_tx, mut stats_rx) = mpsc::unbounded_channel();
    let mut opts = ReceiveOptions::new(target.path().to_path_buf(), staging.path().to_path_buf());
    opts.stats = Some(stats_tx);
    let (peer, receiver) = spawn_receiver("concurrent", opts).await;
    let (_ep, conn) = connect(&peer).await;

    transfer::send(&conn, &[src.path().to_path_buf()], &sender_info(&peer))
        .await
        .expect("send succeeds");
    tokio::time::timeout(Duration::from_secs(60), receiver)
        .await
        .expect("receiver finishes")
        .expect("alive")
        .expect("receive ok");

    let stats = stats_rx.recv().await.expect("stats");
    assert!(
        stats.peak_streams >= 2,
        "parallel striping must hold >= 2 streams open concurrently, peak was {}",
        stats.peak_streams
    );
}

#[tokio::test]
async fn progress_events_cover_every_file_and_byte() {
    let src = TempDir::new().unwrap();
    fs::write(src.path().join("a.bin"), pseudo_bytes(11, 512 * 1024)).unwrap();
    fs::write(src.path().join("b.txt"), b"small").unwrap();
    fs::create_dir_all(src.path().join("nested")).unwrap();
    fs::write(
        src.path().join("nested/c.bin"),
        pseudo_bytes(12, 256 * 1024),
    )
    .unwrap();

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver(
        "events",
        ReceiveOptions::new(target.path().to_path_buf(), staging.path().to_path_buf()),
    )
    .await;
    let (_ep, conn) = connect(&peer).await;

    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut opts = SendOptions::new();
    opts.progress = Some(event_tx.clone());
    let inputs = [
        src.path().join("a.bin"),
        src.path().join("b.txt"),
        src.path().join("nested/c.bin"),
    ];
    transfer::send_with(&conn, &inputs, &sender_info(&peer), &opts)
        .await
        .expect("send succeeds");
    drop(event_tx);
    tokio::time::timeout(Duration::from_secs(30), receiver)
        .await
        .expect("receiver finishes")
        .expect("alive")
        .expect("receive ok");

    let mut started = Vec::new();
    let mut sent_files = Vec::new();
    let mut chunk_bytes: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    while let Ok(event) = event_rx.try_recv() {
        match event {
            TransferEvent::FileStarted { path, size } => {
                started.push((path.clone(), size));
            }
            TransferEvent::ChunkSent { path, bytes, .. } => {
                *chunk_bytes.entry(path).or_default() += bytes;
            }
            TransferEvent::FileSent { path } => sent_files.push(path),
        }
    }
    let expected: Vec<(String, u64)> = vec![
        ("a.bin".into(), 512 * 1024),
        ("b.txt".into(), 5),
        ("c.bin".into(), 256 * 1024),
    ];
    assert_eq!(started.len(), 3);
    for (path, size) in &expected {
        assert!(
            started.contains(&(path.clone(), *size)),
            "no FileStarted for {path}"
        );
        assert_eq!(
            chunk_bytes.get(path),
            Some(size),
            "byte accounting off for {path}"
        );
        assert!(sent_files.contains(path), "no FileSent for {path}");
    }
}

/// Rogue sender: completes file A fully (receiver finalizes it), then
/// cancels. A must stay in the target; B must not exist.
#[tokio::test]
async fn cancelled_session_keeps_completed_files() {
    let src = TempDir::new().unwrap();
    fs::write(src.path().join("a.dat"), pseudo_bytes(21, 512 * 1024)).unwrap();
    fs::write(src.path().join("b.dat"), pseudo_bytes(22, 512 * 1024)).unwrap();
    let manifest = build_manifest(
        &[src.path().join("a.dat"), src.path().join("b.dat")],
        &ChunkParams::default(),
    )
    .unwrap();

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver(
        "cancel",
        ReceiveOptions::new(target.path().to_path_buf(), staging.path().to_path_buf()),
    )
    .await;
    let (_ep, conn) = connect(&peer).await;

    let (mut ctl_tx, mut ctl_rx) = conn.open_bi().await.unwrap();
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Hello {
            name: "rogue".into(),
            fingerprint: peer.fingerprint.clone(),
        }),
    )
    .await
    .unwrap();
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Offer {
            manifest: manifest.clone(),
        }),
    )
    .await
    .unwrap();
    match read_message(&mut ctl_rx).await.unwrap().message {
        Message::Accept { .. } => {}
        other => panic!("expected Accept, got {other:?}"),
    }
    // File A only, fully; then cancel.
    let (mut data_tx, _data_rx) = conn.open_bi().await.unwrap();
    let mut source = fs::File::open(src.path().join("a.dat")).unwrap();
    for (ci, chunk) in manifest.files[0].chunks.iter().enumerate() {
        let mut buf = vec![0u8; chunk.length as usize];
        use std::io::{Read, Seek, SeekFrom};
        source.seek(SeekFrom::Start(chunk.offset)).unwrap();
        source.read_exact(&mut buf).unwrap();
        write_data_frame(&mut data_tx, 0, ci as u32, &buf)
            .await
            .unwrap();
    }
    data_tx.finish().unwrap();
    // Data and control ride separate streams with no cross-stream ordering:
    // on a fast scheduler the Cancel can outrun the last chunk's verification.
    // Wait until all of A is staged (verified bytes hit disk) before
    // cancelling — "completed file A survives" is then the honest assertion.
    let a_bytes: u64 = manifest.files[0].chunks.iter().map(|c| c.length).sum();
    let staged_deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while transfer::staged_bytes(staging.path(), &manifest.hash) < a_bytes {
        assert!(
            tokio::time::Instant::now() < staged_deadline,
            "file A never fully staged before the deadline"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Cancel {
            reason: "changed my mind".into(),
        }),
    )
    .await
    .unwrap();
    // Keep the connection open so the receiver reads the Cancel (not a close).
    let result = tokio::time::timeout(Duration::from_secs(30), receiver)
        .await
        .expect("receiver finishes")
        .expect("alive");
    match result {
        Err(TransferError::Cancelled(reason)) => assert_eq!(reason, "changed my mind"),
        other => panic!("expected Cancelled, got {other:?}"),
    }
    // A completed before the cancel: finalized and stays.
    let a = target.path().join("a.dat");
    assert!(a.is_file(), "completed file must survive cancellation");
    assert_eq!(
        hash_reader(fs::File::open(&a).unwrap()).unwrap(),
        manifest.files[0].hash
    );
    // B was never sent: absent from the target.
    assert!(!target.path().join("b.dat").exists());
}

#[tokio::test]
async fn existing_files_are_never_overwritten() {
    let src = TempDir::new().unwrap();
    fs::write(src.path().join("clip.dat"), pseudo_bytes(31, 128 * 1024)).unwrap();

    let target = TempDir::new().unwrap();
    let original = "ORIGINAL \u{2014} do not touch".as_bytes();
    fs::write(target.path().join("clip.dat"), original).unwrap();

    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver(
        "collision",
        ReceiveOptions::new(target.path().to_path_buf(), staging.path().to_path_buf()),
    )
    .await;
    let (_ep, conn) = connect(&peer).await;

    transfer::send(&conn, &[src.path().join("clip.dat")], &sender_info(&peer))
        .await
        .expect("send succeeds");
    tokio::time::timeout(Duration::from_secs(30), receiver)
        .await
        .expect("receiver finishes")
        .expect("alive")
        .expect("receive ok");

    assert_eq!(fs::read(target.path().join("clip.dat")).unwrap(), original);
    let copies: Vec<_> = fs::read_dir(target.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert!(
        copies
            .iter()
            .any(|n| n.starts_with("clip (") && n.ends_with(").dat")),
        "expected a collision-safe copy, got {copies:?}"
    );
}

/// Corrupt one chunk of the SECOND file; the typed error must name it.
#[tokio::test]
async fn corrupted_chunk_error_identifies_the_file() {
    let src = TempDir::new().unwrap();
    fs::write(src.path().join("first.dat"), pseudo_bytes(41, 256 * 1024)).unwrap();
    fs::write(src.path().join("second.dat"), pseudo_bytes(42, 256 * 1024)).unwrap();
    let manifest = build_manifest(
        &[src.path().join("first.dat"), src.path().join("second.dat")],
        &ChunkParams::default(),
    )
    .unwrap();
    let second_idx = manifest
        .files
        .iter()
        .position(|f| f.path == "second.dat")
        .unwrap();

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver(
        "identify",
        ReceiveOptions::new(target.path().to_path_buf(), staging.path().to_path_buf()),
    )
    .await;
    let (_ep, conn) = connect(&peer).await;

    let (mut ctl_tx, mut ctl_rx) = conn.open_bi().await.unwrap();
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Hello {
            name: "rogue".into(),
            fingerprint: peer.fingerprint.clone(),
        }),
    )
    .await
    .unwrap();
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Offer {
            manifest: manifest.clone(),
        }),
    )
    .await
    .unwrap();
    match read_message(&mut ctl_rx).await.unwrap().message {
        Message::Accept { .. } => {}
        other => panic!("expected Accept, got {other:?}"),
    }
    let (mut data_tx, _data_rx) = conn.open_bi().await.unwrap();
    let mut sent_first = false;
    for (fi, file) in manifest.files.iter().enumerate() {
        let mut source = fs::File::open(src.path().join(&file.path)).unwrap();
        for (ci, chunk) in file.chunks.iter().enumerate() {
            let mut buf = vec![0u8; chunk.length as usize];
            use std::io::{Read, Seek, SeekFrom};
            source.seek(SeekFrom::Start(chunk.offset)).unwrap();
            source.read_exact(&mut buf).unwrap();
            if fi == second_idx && ci == 0 {
                buf[0] ^= 0xFF;
            }
            write_data_frame(&mut data_tx, fi as u32, ci as u32, &buf)
                .await
                .unwrap();
            if fi == second_idx && ci == 0 {
                sent_first = true;
            }
        }
    }
    let _ = sent_first;
    drop(data_tx);
    tokio::time::sleep(Duration::from_millis(400)).await;
    transport::Transport::close(&conn, 12, b"done");

    let result = tokio::time::timeout(Duration::from_secs(30), receiver)
        .await
        .expect("receiver finishes")
        .expect("alive");
    match result {
        Err(TransferError::ChunkHashMismatch { path, .. }) => {
            assert_eq!(path, "second.dat", "error must identify the corrupted file");
        }
        other => panic!("expected ChunkHashMismatch for second.dat, got {other:?}"),
    }
    assert!(!target.path().join("second.dat").exists());
}
