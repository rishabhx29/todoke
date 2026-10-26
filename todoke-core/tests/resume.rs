//! Ticket 07 acceptance tests: resume across reconnects and restarts,
//! corrupt-chunk re-request rounds, and staging bounding.

use std::{fs, path::PathBuf, time::Duration};

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
    let dir = std::env::temp_dir().join(format!("dasho-resume-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_identity(tag: &str) -> (DeviceIdentity, transport::ServerIdentity, String) {
    let id = DeviceIdentity::load_or_create(&tmpdir(tag)).unwrap();
    let server_id = transport::server_identity(&id).unwrap();
    let fp = id.fingerprint();
    (id, server_id, fp)
}

/// Receiver endpoint that serves `sessions` consecutive receive sessions with
/// the same target/staging (simulating a restarted receiver process). Each
/// session's result is pushed to the channel as it finishes, so the test can
/// dial the next session without waiting for all of them.
async fn serve_sessions(
    server_id: transport::ServerIdentity,
    opts: ReceiveOptions,
    sessions: usize,
) -> (
    std::net::SocketAddr,
    mpsc::UnboundedReceiver<Result<todoke_core::manifest::Receipt, TransferError>>,
) {
    let (endpoint, addr) = transport::serve(&server_id, "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        for _ in 0..sessions {
            let incoming = endpoint.accept().await.expect("incoming");
            let conn = incoming.await.expect("handshake");
            let _ = tx.send(transfer::receive(&conn, &opts).await);
        }
    });
    (addr, rx)
}

async fn dial(
    peer_addr: std::net::SocketAddr,
    fp: &str,
    cert: &CertificateDer<'static>,
) -> (quinn::Endpoint, quinn::Connection) {
    transport::dial(peer_addr, fp, cert.clone())
        .await
        .expect("connect")
}

/// A raw protocol sender that writes the first `send_chunks` chunks of the
/// manifest's file, then abruptly dies (no FIN, no Complete).
async fn rogue_partial(
    conn: &quinn::Connection,
    peer_fp: &str,
    manifest: &todoke_core::manifest::Manifest,
    source: &PathBuf,
    send_chunks: usize,
) {
    let (mut ctl_tx, mut ctl_rx) = conn.open_bi().await.unwrap();
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Hello {
            name: "rogue".into(),
            fingerprint: peer_fp.to_string(),
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
    let mut f = fs::File::open(source).unwrap();
    use std::io::{Read, Seek, SeekFrom};
    for (ci, chunk) in manifest.files[0]
        .chunks
        .iter()
        .take(send_chunks)
        .enumerate()
    {
        let mut buf = vec![0u8; chunk.length as usize];
        f.seek(SeekFrom::Start(chunk.offset)).unwrap();
        f.read_exact(&mut buf).unwrap();
        write_data_frame(&mut data_tx, 0, ci as u32, &buf)
            .await
            .unwrap();
    }
    // Give the receiver time to drain every queued frame, then die without
    // FIN (a real crash happens after data has flowed for a while).
    tokio::time::sleep(Duration::from_millis(1200)).await;
    transport::Transport::close(conn, 9, b"crashed");
}

#[tokio::test]
async fn resume_completes_and_skips_already_received_chunks() {
    let src = TempDir::new().unwrap();
    let file = src.path().join("big.dat");
    fs::write(&file, pseudo_bytes(77, 8 * 1024 * 1024)).unwrap();
    let manifest = build_manifest(std::slice::from_ref(&file), &ChunkParams::default()).unwrap();
    let total_chunks: usize = manifest.files[0].chunks.len();
    let partial = total_chunks / 3;

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (_id, server_id, fp) = make_identity("resume");
    let cert = CertificateDer::from(server_id.cert_der().to_vec());
    let opts = ReceiveOptions::new(target.path().to_path_buf(), staging.path().to_path_buf());
    let (addr, mut receiver) = serve_sessions(server_id, opts, 2).await;

    // Session 1: rogue sends a third, then crashes. Receiver must fail.
    {
        let (_ep, conn) = dial(addr, &fp, &cert).await;
        rogue_partial(&conn, &fp, &manifest, &file, partial).await;
    }
    let first = tokio::time::timeout(Duration::from_secs(30), receiver.recv())
        .await
        .expect("session 1 done")
        .expect("receiver alive");
    assert!(first.is_err(), "crashed session must fail: {first:?}");

    // Session 2: a full send() reconciles against staging and completes.
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut send_opts = SendOptions::new();
    send_opts.progress = Some(event_tx.clone());
    {
        let (_ep, conn) = dial(addr, &fp, &cert).await;
        transfer::send_with(
            &conn,
            std::slice::from_ref(&file),
            &SenderInfo {
                name: "resumer".into(),
                fingerprint: fp.clone(),
            },
            &send_opts,
        )
        .await
        .expect("resumed send succeeds");
    }
    let second = tokio::time::timeout(Duration::from_secs(60), receiver.recv())
        .await
        .expect("session 2 done")
        .expect("receiver alive");
    assert!(second.is_ok(), "resumed session must succeed: {second:?}");

    // Byte-identical.
    let got = target.path().join("big.dat");
    assert_eq!(
        hash_reader(fs::File::open(&got).unwrap()).unwrap(),
        manifest.files[0].hash
    );

    // Proof of reconciliation: the resumed session sent only the remainder.
    drop(event_tx);
    let mut chunk_sends = 0usize;
    while let Ok(event) = event_rx.try_recv() {
        if matches!(event, TransferEvent::ChunkSent { .. }) {
            chunk_sends += 1;
        }
    }
    assert_eq!(
        chunk_sends,
        total_chunks - partial,
        "resumed session must skip the {partial} chunks already staged (total {total_chunks})"
    );

    // The transfer report: staging cleaned up for the completed manifest.
    assert_eq!(transfer::staged_bytes(staging.path(), &manifest.hash), 0);
}

/// Sender that delivers every chunk but tampers with one, then completes.
/// The receiver must answer Missing; the corrected resend completes the
/// transfer — all within one protocol session.
#[tokio::test]
async fn corrupt_chunk_is_rerequested_and_transfer_completes() {
    let src = TempDir::new().unwrap();
    let file = src.path().join("payload.dat");
    fs::write(&file, pseudo_bytes(101, 2 * 1024 * 1024)).unwrap();
    let manifest = build_manifest(std::slice::from_ref(&file), &ChunkParams::default()).unwrap();
    let tamper_index = 1usize;

    let target = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();
    let (_id, server_id, fp) = make_identity("rerequest");
    let cert = CertificateDer::from(server_id.cert_der().to_vec());
    let opts = ReceiveOptions::new(target.path().to_path_buf(), staging.path().to_path_buf());
    let (endpoint, addr) = transport::serve(&server_id, "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let receiver = tokio::spawn(async move {
        let incoming = endpoint.accept().await.expect("incoming");
        let conn = incoming.await.expect("handshake");
        transfer::receive(&conn, &opts).await
    });

    let (_ep, conn) = dial(addr, &fp, &cert).await;
    let (mut ctl_tx, mut ctl_rx) = conn.open_bi().await.unwrap();
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Hello {
            name: "careless".into(),
            fingerprint: fp.clone(),
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

    // (round 1 uses ctl_tx directly)
    {
        let (data_tx, _data_rx) = conn.open_bi().await.unwrap();
        let mut f = fs::File::open(&file).unwrap();
        use std::io::{Read, Seek, SeekFrom};
        let mut data_tx = data_tx;
        for (ci, chunk) in manifest.files[0].chunks.iter().enumerate() {
            let mut buf = vec![0u8; chunk.length as usize];
            f.seek(SeekFrom::Start(chunk.offset)).unwrap();
            f.read_exact(&mut buf).unwrap();
            if Some(ci) == Some(tamper_index) {
                buf[0] ^= 0xFF;
            }
            write_data_frame(&mut data_tx, 0, ci as u32, &buf)
                .await
                .unwrap();
        }
        data_tx.finish().unwrap();
        write_message(
            &mut ctl_tx,
            &Envelope::new(Message::Complete {
                manifest_hash: manifest.hash.clone(),
            }),
        )
        .await
        .unwrap();
    }

    // The receiver answers Missing for the tampered chunk.
    match read_message(&mut ctl_rx).await.unwrap().message {
        Message::Missing { chunks, .. } => {
            assert_eq!(chunks.len(), 1);
            assert_eq!(chunks[0].chunk as usize, tamper_index);
        }
        other => panic!("expected Missing, got {other:?}"),
    }

    // Round 2: resend the missing chunk correctly, complete again.
    {
        let (mut data_tx, _data_rx) = conn.open_bi().await.unwrap();
        let chunk = &manifest.files[0].chunks[tamper_index];
        let mut buf = vec![0u8; chunk.length as usize];
        let mut f = fs::File::open(&file).unwrap();
        use std::io::{Read, Seek, SeekFrom};
        f.seek(SeekFrom::Start(chunk.offset)).unwrap();
        f.read_exact(&mut buf).unwrap();
        write_data_frame(&mut data_tx, 0, tamper_index as u32, &buf)
            .await
            .unwrap();
        data_tx.finish().unwrap();
        write_message(
            &mut ctl_tx,
            &Envelope::new(Message::Complete {
                manifest_hash: manifest.hash.clone(),
            }),
        )
        .await
        .unwrap();
    }
    match read_message(&mut ctl_rx).await.unwrap().message {
        Message::Receipt { receipt } => {
            assert!(receipt.verify_against(&manifest));
            assert_eq!(receipt.files[0].hash, manifest.files[0].hash);
        }
        other => panic!("expected Receipt after re-request, got {other:?}"),
    }

    let got = target.path().join("payload.dat");
    assert_eq!(
        hash_reader(fs::File::open(&got).unwrap()).unwrap(),
        manifest.files[0].hash
    );
    let result = tokio::time::timeout(Duration::from_secs(30), receiver)
        .await
        .expect("receiver finishes")
        .expect("alive");
    assert!(result.is_ok(), "receiver must succeed: {result:?}");
}

#[tokio::test]
async fn staging_is_bounded_across_aborted_manifests() {
    // Litter the staging root with 12 stale manifest dirs.
    let staging = TempDir::new().unwrap();
    for i in 0..12u32 {
        let dir = staging.path().join(format!("{i:016x}"));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("junk"), b"x").unwrap();
    }

    let src = TempDir::new().unwrap();
    let file = src.path().join("small.dat");
    fs::write(&file, pseudo_bytes(5, 64 * 1024)).unwrap();

    let target = TempDir::new().unwrap();
    let (_id, server_id, fp) = make_identity("sweep");
    let cert = CertificateDer::from(server_id.cert_der().to_vec());
    let opts = ReceiveOptions::new(target.path().to_path_buf(), staging.path().to_path_buf());
    let (addr, mut receiver) = serve_sessions(server_id, opts, 1).await;
    {
        let (_ep, conn) = dial(addr, &fp, &cert).await;
        transfer::send(
            &conn,
            &[file],
            &SenderInfo {
                name: "s".into(),
                fingerprint: fp,
            },
        )
        .await
        .expect("send succeeds");
    }
    let first = tokio::time::timeout(Duration::from_secs(30), receiver.recv())
        .await
        .expect("receiver finishes")
        .expect("receiver alive");
    assert!(first.is_ok(), "receive must succeed: {first:?}");

    let dirs = fs::read_dir(staging.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .count();
    assert!(
        dirs <= 8,
        "staging must stay bounded (8 newest + sweep), found {dirs}"
    );
}
