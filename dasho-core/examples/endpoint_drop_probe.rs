//! Probe: does a quinn connection survive its client endpoint being dropped?
use dasho_core::{identity::DeviceIdentity, transport};
use rustls::pki_types::CertificateDer;
use tempfile::TempDir;

#[tokio::main]
async fn main() {
    let dir = TempDir::new().unwrap();
    let id = DeviceIdentity::load_or_create(dir.path()).unwrap();
    let server_id = transport::server_identity(&id).unwrap();
    let fp = id.fingerprint();
    let cert = CertificateDer::from(server_id.cert_der().to_vec());

    let (endpoint, addr) = transport::serve(&server_id, "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    tokio::spawn(async move {
        if let Some(incoming) = endpoint.accept().await {
            if let Ok(conn) = incoming.await {
                if let Ok((mut tx, _rx)) = conn.accept_bi().await {
                    let _ = tx.write_all(b"pong").await;
                    let _ = tx.finish();
                }
            }
        }
    });

    // Case: endpoint dropped right after connect.
    let conn = {
        let client = transport::client_endpoint().unwrap();
        transport::connect(&client, addr, &fp, cert).await.unwrap()
    };
    println!("connected");
    match conn.open_bi().await {
        Ok((mut tx, mut rx)) => {
            let sent = tx.write_all(b"ping").await;
            let fin = tx.finish();
            println!("write: {sent:?}, finish: {fin:?}");
            let mut buf = [0u8; 4];
            match tokio::time::timeout(std::time::Duration::from_secs(3), rx.read_exact(&mut buf))
                .await
            {
                Ok(Ok(_)) => println!("CASE B (endpoint dropped): roundtrip OK -> {buf:?}"),
                Ok(Err(e)) => println!("CASE B (endpoint dropped): stream error {e}"),
                Err(_) => println!("CASE B (endpoint dropped): TIMEOUT"),
            }
        }
        Err(e) => println!("CASE B (endpoint dropped): open_bi failed: {e}"),
    }
}
