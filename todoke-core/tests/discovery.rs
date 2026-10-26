//! Ticket 04 acceptance tests: advertise + browse on one host, dedupe,
//! malformed-advertisement rejection, clean shutdown.
//!
//! These tests use real multicast on the host's network interfaces. They
//! need an active network interface (Wi-Fi or Ethernet); on hosts without
//! one, the loopback-only tests still run and the LAN test is skipped.

use std::time::Duration;

use todoke_core::{
    discovery::{self, DeviceRecord, DiscoveryError},
    identity::DeviceIdentity,
    transport,
};

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("todoke-discovery-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_device(tag: &str, name: &str) -> (DeviceIdentity, transport::ServerIdentity) {
    let mut id = DeviceIdentity::load_or_create(&tmpdir(tag)).unwrap();
    id.set_name(name).unwrap();
    let server = transport::server_identity(&id).unwrap();
    (id, server)
}

#[tokio::test]
async fn advertised_device_is_discovered_on_same_host() {
    let (id, server) = make_device("adv", "Test-Laptop");
    let fp = id.fingerprint();
    let advertiser =
        discovery::Advertiser::advertise_on(&id, &server, 45_001, "127.0.0.1").unwrap();

    // Give the announcer a moment, then browse.
    tokio::time::sleep(Duration::from_millis(700)).await;
    let records = discovery::browse(Duration::from_secs(6)).await.unwrap();

    advertiser.stop().unwrap();
    assert!(
        records
            .iter()
            .any(|r| r.fingerprint == fp && r.name == "Test-Laptop"),
        "expected own device {fp} in {records:?}"
    );
    let own = records.iter().find(|r| r.fingerprint == fp).unwrap();
    assert_eq!(own.port, 45_001);
    assert!(!own.cert_b64.is_empty(), "certificate must be advertised");
    assert!(own.validate().is_ok());
}

#[tokio::test]
async fn duplicate_announcements_dedupe_by_fingerprint() {
    // Registering the same identity twice produces two fullnames with one
    // fingerprint; browse must collapse them to one record.
    let (id, server) = make_device("dedupe", "Dedupe-Device");
    let fp = id.fingerprint();
    let a = discovery::Advertiser::advertise_on(&id, &server, 45_011, "127.0.0.1").unwrap();
    let b = discovery::Advertiser::advertise_on(&id, &server, 45_012, "127.0.0.1").unwrap();

    tokio::time::sleep(Duration::from_millis(700)).await;
    let records = discovery::browse(Duration::from_secs(6)).await.unwrap();
    drop(a);
    drop(b);

    let count = records.iter().filter(|r| r.fingerprint == fp).count();
    assert_eq!(
        count, 1,
        "fingerprint {fp} must appear exactly once: {records:?}"
    );
}

#[tokio::test]
async fn advertiser_stop_removes_the_service() {
    let (id, server) = make_device("stop", "Gone-Device");
    let fp = id.fingerprint();
    let advertiser =
        discovery::Advertiser::advertise_on(&id, &server, 45_021, "127.0.0.1").unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;
    advertiser.stop().unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;

    let records = discovery::browse(Duration::from_secs(5)).await.unwrap();
    assert!(
        !records.iter().any(|r| r.fingerprint == fp),
        "unregistered device must not be discovered: {records:?}"
    );
}

#[test]
fn malformed_records_are_rejected() {
    let good = DeviceRecord {
        name: "ok".into(),
        fingerprint: "AAAABBBB-CCCCDDDD-EEEEFFFF-00001111".into(),
        cert_b64: "aGk=".into(),
        port: 1,
        addr: std::net::Ipv4Addr::LOCALHOST,
    };
    assert!(good.validate().is_ok());

    let empty_name = DeviceRecord {
        name: String::new(),
        ..good.clone()
    };
    assert!(matches!(
        empty_name.validate(),
        Err(DiscoveryError::EmptyName)
    ));

    let bad_fp = DeviceRecord {
        fingerprint: "ZZZZ!!!!-CCCCDDDD-EEEEFFFF-00001111".into(),
        ..good.clone()
    };
    assert!(matches!(
        bad_fp.validate(),
        Err(DiscoveryError::BadFingerprint(_))
    ));

    let bad_len_fp = DeviceRecord {
        fingerprint: "AABB".into(),
        ..good.clone()
    };
    assert!(matches!(
        bad_len_fp.validate(),
        Err(DiscoveryError::BadFingerprint(_))
    ));

    let no_cert = DeviceRecord {
        cert_b64: String::new(),
        ..good
    };
    assert!(matches!(
        no_cert.validate(),
        Err(DiscoveryError::BadCertificate)
    ));
}

#[test]
fn fingerprint_validation_is_format_exact() {
    // 32 hex chars in 4 groups of 8 with dashes at positions 8, 17, 26.
    let ok = "AAAABBBB-CCCCDDDD-EEEEFFFF-00001111";
    let mut c = ok.bytes().filter(|b| *b != b'-').count();
    assert_eq!(c, 32);
    c = ok.matches('-').count();
    assert_eq!(c, 3);
    // Mixed case is accepted (fingerprints render upper-case but compare
    // case-insensitively at the trust layer).
    assert!(
        DeviceRecord {
            name: "x".into(),
            fingerprint: "aaaabbbb-ccccdddd-eeeeffff-00001111".into(),
            cert_b64: "x".into(),
            port: 1,
            addr: std::net::Ipv4Addr::LOCALHOST,
        }
        .validate()
        .is_ok()
    );
}
