//! Ticket 05 acceptance tests: end-to-end transfers over loopback QUIC —
//! one large file, staged finalization, receipts, and failure semantics.

use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    net::SocketAddr,
    path::PathBuf,
    time::Duration,
};

use rustls::pki_types::CertificateDer;
use tempfile::TempDir;
use todoke_core::{
    chunk::{ChunkParams, EMPTY_BLAKE3, hash_reader},
    identity::DeviceIdentity,
    manifest::{Manifest, Receipt, build_manifest},
    protocol::{Envelope, Message, read_message, write_data_frame, write_message},
    transfer::{self, ReceiveOptions, SenderInfo, TransferError},
    transport::{self, Transport},
};

/// Deterministic pseudo-random bytes (splitmix64) so tests never depend on
/// entropy; content-defined chunking needs varied data to cut realistically.
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
    let dir = std::env::temp_dir().join(format!("todoke-transfer-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

struct Peer {
    fingerprint: String,
    addr: SocketAddr,
    cert: CertificateDer<'static>,
}

/// Spawn a QUIC server that runs exactly one receive session and reports
/// its outcome via the returned join handle.
async fn spawn_receiver_once(
    tag: &str,
    target: PathBuf,
    staging: PathBuf,
) -> (
    Peer,
    tokio::task::JoinHandle<Result<Receipt, TransferError>>,
) {
    let id = DeviceIdentity::load_or_create(&tmpdir(tag)).unwrap();
    let server_id = transport::server_identity(&id).unwrap();
    let fingerprint = id.fingerprint();
    let cert = CertificateDer::from(server_id.cert_der().to_vec());
    let (endpoint, addr) = transport::serve(&server_id, "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();

    let opts = ReceiveOptions::new(target, staging);
    let handle = tokio::spawn(async move {
        let incoming = endpoint.accept().await.expect("incoming connection");
        let conn = incoming.await.expect("QUIC handshake");
        transfer::receive(&conn, &opts).await
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

/// Returns the endpoint too — a quinn connection dies when its endpoint's
/// driver is dropped, so the caller must keep both alive.
async fn connect(peer: &Peer) -> (quinn::Endpoint, quinn::Connection) {
    transport::dial(peer.addr, &peer.fingerprint, peer.cert.clone())
        .await
        .expect("connect")
}

fn sender_info(peer: &Peer) -> SenderInfo {
    SenderInfo {
        name: "test-sender".into(),
        fingerprint: peer.fingerprint.clone(),
    }
}

#[tokio::test]
async fn hundred_mb_file_round_trips_with_verified_receipt() {
    let src = TempDir::new().unwrap();
    let big = src.path().join("lecture-recording.dat");
    fs::write(&big, pseudo_bytes(0xD05D0, 100 * 1024 * 1024)).unwrap();
    let expected_hash = hash_reader(fs::File::open(&big).unwrap()).unwrap();

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver_once(
        "big",
        target.path().to_path_buf(),
        staging.path().to_path_buf(),
    )
    .await;
    let (_client_ep, conn) = connect(&peer).await;

    let report = tokio::time::timeout(
        Duration::from_secs(300),
        transfer::send(&conn, std::slice::from_ref(&big), &sender_info(&peer)),
    )
    .await
    .expect("send finishes within 5 minutes")
    .expect("send succeeds");

    let receipt = tokio::time::timeout(Duration::from_secs(60), receiver)
        .await
        .expect("receiver finishes")
        .expect("receiver task alive")
        .expect("receive succeeds");

    // Sender-side verification already ran inside send(); assert explicitly
    // for the acceptance criterion.
    assert!(receipt.verify_against(&report.manifest));
    assert_eq!(receipt, report.receipt, "both sides saw the same receipt");

    // Byte-identical on disk.
    let received = target.path().join("lecture-recording.dat");
    assert_eq!(fs::metadata(&received).unwrap().len(), 100 * 1024 * 1024);
    assert_eq!(
        hash_reader(fs::File::open(&received).unwrap()).unwrap(),
        expected_hash
    );

    // Receipt content: path, size, whole-file hash, chunk count.
    assert_eq!(receipt.files.len(), 1);
    let rf = &receipt.files[0];
    assert_eq!(rf.path, "lecture-recording.dat");
    assert_eq!(rf.size, 100 * 1024 * 1024);
    assert_eq!(rf.hash, expected_hash);
    // 100 MiB / 256 KiB max chunk = 400: at least 400 chunks must exist.
    assert!(rf.chunks >= 400, "chunk count {} too low", rf.chunks);

    // Finalized transfers clean up their staging.
    let stage_key: String = report.manifest.hash.chars().take(16).collect();
    assert!(
        !staging.path().join(stage_key).exists(),
        "staging must be cleaned"
    );
    assert_eq!(
        transfer::staged_bytes(staging.path(), &report.manifest.hash),
        0
    );
}

#[tokio::test]
async fn small_tree_round_trips_with_structure_unicode_and_empty_edges() {
    let src = TempDir::new().unwrap();
    let bundle = src.path().join("bundle");
    fs::create_dir_all(bundle.join("docs")).unwrap();
    fs::create_dir_all(bundle.join("photos/2026")).unwrap(); // stays empty
    fs::write(
        bundle.join("docs/über-space report.txt"),
        b"unicode names welcome",
    )
    .unwrap();
    fs::write(bundle.join("docs/empty.bin"), b"").unwrap();
    fs::write(bundle.join("clip.dat"), pseudo_bytes(5, 300 * 1024)).unwrap();
    let manifest = build_manifest(std::slice::from_ref(&bundle), &ChunkParams::default()).unwrap();
    assert_eq!(manifest.empty_dirs, vec!["bundle/photos/2026".to_string()]);

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver_once(
        "tree",
        target.path().to_path_buf(),
        staging.path().to_path_buf(),
    )
    .await;
    let (_client_ep, conn) = connect(&peer).await;

    let report = tokio::time::timeout(
        Duration::from_secs(60),
        transfer::send(&conn, &[bundle], &sender_info(&peer)),
    )
    .await
    .expect("send finishes")
    .expect("send succeeds");
    tokio::time::timeout(Duration::from_secs(60), receiver)
        .await
        .expect("receiver finishes")
        .expect("receiver task alive")
        .expect("receive succeeds");

    // Every file byte-identical at the same relative path.
    for entry in &report.manifest.files {
        let got = target.path().join(&entry.path);
        assert!(got.is_file(), "missing {}", got.display());
        assert_eq!(
            fs::metadata(&got).unwrap().len(),
            entry.size,
            "{}",
            entry.path
        );
        assert_eq!(
            hash_reader(fs::File::open(&got).unwrap()).unwrap(),
            entry.hash,
            "{} changed in transit",
            entry.path
        );
    }
    // Empty file materialized with its known constant hash.
    let empty = report
        .manifest
        .files
        .iter()
        .find(|f| f.path.ends_with("empty.bin"))
        .unwrap();
    assert_eq!(empty.hash, EMPTY_BLAKE3);
    assert_eq!(empty.chunks.len(), 0);
    // Empty directory recreated.
    assert!(target.path().join("bundle/photos/2026").is_dir());
}

#[tokio::test]
async fn empty_single_file_transfers_cleanly() {
    let src = TempDir::new().unwrap();
    let file = src.path().join("empty.txt");
    fs::write(&file, b"").unwrap();

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver_once(
        "empty",
        target.path().to_path_buf(),
        staging.path().to_path_buf(),
    )
    .await;
    let (_client_ep, conn) = connect(&peer).await;

    let report = tokio::time::timeout(
        Duration::from_secs(30),
        transfer::send(&conn, &[file], &sender_info(&peer)),
    )
    .await
    .expect("send finishes")
    .expect("send succeeds");
    let receipt = tokio::time::timeout(Duration::from_secs(30), receiver)
        .await
        .expect("receiver finishes")
        .expect("receiver task alive")
        .expect("receive succeeds");

    let got = target.path().join("empty.txt");
    assert!(got.is_file());
    assert_eq!(fs::metadata(&got).unwrap().len(), 0);
    assert_eq!(receipt.files[0].hash, EMPTY_BLAKE3);
    assert_eq!(receipt.files[0].chunks, 0);
    assert!(receipt.verify_against(&report.manifest));
}

#[tokio::test]
async fn dropped_sender_mid_transfer_leaves_target_untouched() {
    let src = TempDir::new().unwrap();
    let file = src.path().join("partial.dat");
    fs::write(&file, pseudo_bytes(77, 8 * 1024 * 1024)).unwrap();
    let manifest = build_manifest(std::slice::from_ref(&file), &ChunkParams::default()).unwrap();

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver_once(
        "drop",
        target.path().to_path_buf(),
        staging.path().to_path_buf(),
    )
    .await;
    let (_client_ep, conn) = connect(&peer).await;

    // A rogue sender: full handshake, a few chunks, then abrupt teardown —
    // no FIN, no Complete, connection killed outright.
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
    let (mut data_tx, _) = conn.open_bi().await.unwrap();
    let mut source = fs::File::open(&file).unwrap();
    for (ci, chunk) in manifest.files[0].chunks.iter().enumerate().take(5) {
        let mut buf = vec![0u8; chunk.length as usize];
        source.seek(SeekFrom::Start(chunk.offset)).unwrap();
        source.read_exact(&mut buf).unwrap();
        write_data_frame(&mut data_tx, 0, ci as u32, &buf)
            .await
            .unwrap();
    }
    // Let the receiver actually drain the queued frames before the crash —
    // a real kill happens after data flowed; quinn discards unacked data on close.
    tokio::time::sleep(Duration::from_millis(400)).await;
    Transport::close(&conn, 9, b"simulated crash");

    let result = tokio::time::timeout(Duration::from_secs(30), receiver)
        .await
        .expect("receiver finishes")
        .expect("receiver task alive");
    assert!(result.is_err(), "receiver must fail after sender death");
    assert!(
        !target.path().join("partial.dat").exists(),
        "target must stay untouched"
    );
    assert_eq!(
        fs::read_dir(target.path()).unwrap().count(),
        0,
        "nothing partial may appear in the target"
    );
    // What was received stays staged (resumable foundation for ticket 07).
    let staged = transfer::staged_bytes(staging.path(), &manifest.hash);
    assert!(staged > 0, "some chunks were received");
    assert!(staged < manifest.total_bytes, "not everything arrived");
}

#[tokio::test]
async fn corrupted_chunk_is_rejected_and_target_stays_clean() {
    let src = TempDir::new().unwrap();
    let file = src.path().join("payload.dat");
    fs::write(&file, pseudo_bytes(101, 4 * 1024 * 1024)).unwrap();
    let manifest = build_manifest(std::slice::from_ref(&file), &ChunkParams::default()).unwrap();

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver_once(
        "corrupt",
        target.path().to_path_buf(),
        staging.path().to_path_buf(),
    )
    .await;
    let (_client_ep, conn) = connect(&peer).await;

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
    let (mut data_tx, _) = conn.open_bi().await.unwrap();
    let mut source = fs::File::open(&file).unwrap();
    for (ci, chunk) in manifest.files[0].chunks.iter().enumerate().take(4) {
        let mut buf = vec![0u8; chunk.length as usize];
        source.seek(SeekFrom::Start(chunk.offset)).unwrap();
        source.read_exact(&mut buf).unwrap();
        if ci == 2 {
            // Tamper: right length, wrong bytes.
            buf[0] ^= 0xFF;
        }
        write_data_frame(&mut data_tx, 0, ci as u32, &buf)
            .await
            .unwrap();
    }
    drop(data_tx);
    tokio::time::sleep(Duration::from_millis(400)).await;
    Transport::close(&conn, 10, b"done sending");

    let result = tokio::time::timeout(Duration::from_secs(30), receiver)
        .await
        .expect("receiver finishes")
        .expect("receiver task alive");
    match result {
        Err(TransferError::ChunkHashMismatch { path, chunk, .. }) => {
            assert!(path.ends_with("payload.dat"));
            assert_eq!(chunk, 2, "the tampered chunk must be the one flagged");
        }
        other => panic!("expected ChunkHashMismatch, got {other:?}"),
    }
    assert!(!target.path().join("payload.dat").exists());
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
    // The tampered chunk was never written: staged bytes = chunks 0+1+3.
    assert!(transfer::staged_bytes(staging.path(), &manifest.hash) > 0);
}

#[tokio::test]
async fn tampered_manifest_offer_is_rejected() {
    let src = TempDir::new().unwrap();
    let file = src.path().join("liar.txt");
    fs::write(&file, b"trust me").unwrap();
    let mut manifest: Manifest = build_manifest(&[file], &ChunkParams::default()).unwrap();
    manifest.files[0].size = 999; // hash no longer matches content

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (peer, receiver) = spawn_receiver_once(
        "tamper",
        target.path().to_path_buf(),
        staging.path().to_path_buf(),
    )
    .await;
    let (_client_ep, conn) = connect(&peer).await;

    let (mut ctl_tx, mut _ctl_rx) = conn.open_bi().await.unwrap();
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
    drop(ctl_tx);
    // Let the receiver read the queued Hello/Offer before the close frame
    // arrives (quinn discards queued-but-unacked data on close).
    tokio::time::sleep(Duration::from_millis(400)).await;
    Transport::close(&conn, 11, b"bye");

    let result = tokio::time::timeout(Duration::from_secs(30), receiver)
        .await
        .expect("receiver finishes")
        .expect("receiver task alive");
    match result {
        Err(TransferError::RemoteError(msg)) => {
            assert!(msg.contains("hash verification"), "got: {msg}");
        }
        other => panic!("expected RemoteError, got {other:?}"),
    }
    assert_eq!(fs::read_dir(target.path()).unwrap().count(), 0);
}
