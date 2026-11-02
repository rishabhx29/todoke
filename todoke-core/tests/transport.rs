//! Ticket 03 acceptance tests: identity-verified QUIC over loopback.

use std::{net::SocketAddr, time::Duration};

use rustls::pki_types::CertificateDer;
use todoke_core::{
    identity::DeviceIdentity,
    transport::{self},
};

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("todoke-transport-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Spawn an echo server (echoes the first 5 bytes of every bi-stream).
/// Returns its fingerprint, bound address, and certificate DER.
async fn spawn_echo_server(tag: &str) -> (String, SocketAddr, CertificateDer<'static>) {
    let id = DeviceIdentity::load_or_create(&tmpdir(tag)).unwrap();
    let server_id = transport::server_identity(&id).unwrap();
    let fingerprint = id.fingerprint();
    let cert = CertificateDer::from(server_id.cert_der().to_vec());

    let (endpoint, addr) = transport::serve(&server_id, "127.0.0.1:0".parse().unwrap())
        .await
        .expect("serve");
    tokio::spawn(async move {
        while let Some(incoming) = endpoint.accept().await {
            let Ok(conn) = incoming.await else { continue };
            tokio::spawn(async move {
                while let Ok((mut tx, mut rx)) = conn.accept_bi().await {
                    let mut buf = [0u8; 6];
                    if rx.read_exact(&mut buf).await.is_ok() {
                        let _ = tx.write_all(&buf).await;
                        let _ = tx.finish();
                    }
                }
            });
        }
    });
    (fingerprint, addr, cert)
}

#[tokio::test]
async fn identity_verified_handshake_and_byte_exchange() {
    let (fingerprint, addr, cert) = spawn_echo_server("echo").await;

    let client = transport::client_endpoint().unwrap();
    let conn = transport::connect(&client, addr, &fingerprint, cert)
        .await
        .expect("identity-verified connect");

    let (mut tx, mut rx) = conn.open_bi().await.expect("open_bi");
    tx.write_all(b"todoke").await.expect("write");
    tx.finish().expect("finish");
    let mut echoed = [0u8; 6];
    rx.read_exact(&mut echoed).await.expect("read echo");
    assert_eq!(&echoed, b"todoke");
}

#[tokio::test]
async fn four_concurrent_connections() {
    let (fingerprint, addr, cert) = spawn_echo_server("concurrent").await;
    let client = transport::client_endpoint().unwrap();

    let client = std::sync::Arc::new(client);
    let mut tasks = Vec::new();
    for i in 0..4u8 {
        let fp = fingerprint.clone();
        let cert = cert.clone();
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            let conn = transport::connect(&client, addr, &fp, cert)
                .await
                .expect("connect");
            let (mut tx, mut rx) = conn.open_bi().await.expect("open_bi");
            tx.write_all(&[b'a' + i; 6]).await.expect("write");
            tx.finish().expect("finish");
            let mut echoed = [0u8; 6];
            rx.read_exact(&mut echoed).await.expect("read echo");
            assert_eq!(echoed, [b'a' + i; 6]);
        }));
    }
    for t in tasks {
        tokio::time::timeout(Duration::from_secs(15), t)
            .await
            .expect("no timeout")
            .expect("task ok");
    }
}

#[tokio::test]
async fn wrong_pinned_certificate_is_rejected() {
    let (fingerprint, addr, _real_cert) = spawn_echo_server("wrongcert").await;

    // An impostor's certificate: a different identity's self-signed cert.
    let impostor_dir = tmpdir("wrongcert-impostor");
    let impostor = DeviceIdentity::load_or_create(&impostor_dir).unwrap();
    let impostor_server = transport::server_identity(&impostor).unwrap();
    let impostor_cert = CertificateDer::from(impostor_server.cert_der().to_vec());

    let client = transport::client_endpoint().unwrap();
    // Advertised fingerprint says one device; the cert chains to a different
    // key. Pinning the impostor cert as root would make the chain verify, but
    // then the SAN would match the IMPOSTOR's name, not the advertised one —
    // so the handshake must fail either way.
    let result = transport::connect(&client, addr, &fingerprint, impostor_cert).await;
    assert!(result.is_err(), "mismatched cert must not connect");
}

#[tokio::test]
async fn correct_cert_with_wrong_expected_fingerprint_is_rejected() {
    let (_fp, addr, cert) = spawn_echo_server("wrongfp").await;

    // Right certificate, but the client expects a different device (e.g. a
    // stale trust-store entry). The SAN won't match the expected name.
    let client = transport::client_endpoint().unwrap();
    let result =
        transport::connect(&client, addr, "00000000-11111111-22222222-33333333", cert).await;
    assert!(result.is_err(), "SAN/name mismatch must not connect");
}
