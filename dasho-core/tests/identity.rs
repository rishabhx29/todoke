//! Ticket 02 acceptance tests: identity generate/load round-trip, naming,
//! signing, trust store round-trip, mismatch detection, corruption safety.

use dasho_core::identity::{DeviceIdentity, IdentityError, TrustError, TrustStore};

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("dasho-identity-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn generate_then_reload_yields_same_fingerprint() {
    let dir = tmpdir("roundtrip");
    let id = DeviceIdentity::load_or_create(&dir).unwrap();
    let fp1 = id.fingerprint();
    assert_eq!(fp1.len(), 35, "4x8 hex + 3 dashes");
    assert_eq!(fp1.matches('-').count(), 3);

    let id2 = DeviceIdentity::load_or_create(&dir).unwrap();
    assert_eq!(
        fp1,
        id2.fingerprint(),
        "fingerprint must be stable across loads"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_devices_have_different_fingerprints() {
    let a = DeviceIdentity::load_or_create(&tmpdir("distinct-a")).unwrap();
    let b = DeviceIdentity::load_or_create(&tmpdir("distinct-b")).unwrap();
    assert_ne!(a.fingerprint(), b.fingerprint());
}

#[test]
fn sign_and_verify_roundtrip_and_tamper_detection() {
    let dir = tmpdir("sign");
    let id = DeviceIdentity::load_or_create(&dir).unwrap();
    let msg = b"dasho hello";
    let sig = id.sign(msg);
    assert!(DeviceIdentity::verify(&id.public_key(), msg, &sig));
    assert!(!DeviceIdentity::verify(&id.public_key(), b"tampered", &sig));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn rename_persists_without_touching_the_key() {
    let dir = tmpdir("rename");
    let mut id = DeviceIdentity::load_or_create(&dir).unwrap();
    let fp = id.fingerprint();
    id.set_name("Rishabh-Laptop").unwrap();

    let reloaded = DeviceIdentity::load_or_create(&dir).unwrap();
    assert_eq!(reloaded.name(), "Rishabh-Laptop");
    assert_eq!(
        reloaded.fingerprint(),
        fp,
        "renaming must not rotate the key"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn trust_store_roundtrip_and_lookup() {
    let dir = tmpdir("trust");
    let id = DeviceIdentity::load_or_create(&dir).unwrap();
    let mut store = TrustStore::load(&dir).unwrap();
    store
        .record_peer(&dir, "Study-PC", "AAAA1111-BBBB2222-CCCC3333-DDDD4444")
        .unwrap();

    let reloaded = TrustStore::load(&dir).unwrap();
    assert_eq!(reloaded.peers().len(), 1);
    assert!(
        reloaded
            .lookup("AAAA1111-BBBB2222-CCCC3333-DDDD4444")
            .is_some()
    );
    assert!(
        reloaded
            .lookup("00000000-00000000-00000000-00000000")
            .is_none()
    );

    // TOFU: same name + same key verifies clean.
    let mut store = reloaded;
    store
        .verify_or_record(&dir, "Study-PC", "AAAA1111-BBBB2222-CCCC3333-DDDD4444")
        .unwrap();
    // Unknown peer gets recorded on first sight.
    store
        .verify_or_record(&dir, "Phone", id.fingerprint().as_str())
        .unwrap();
    assert_eq!(store.peers().len(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn mismatched_key_for_known_name_is_an_error() {
    let dir = tmpdir("mismatch");
    let mut store = TrustStore::load(&dir).unwrap();
    store
        .record_peer(&dir, "Study-PC", "AAAA1111-BBBB2222-CCCC3333-DDDD4444")
        .unwrap();

    let err = store
        .verify_or_record(&dir, "Study-PC", "EEEE1111-BBBB2222-CCCC3333-DDDD4444")
        .unwrap_err();
    match err {
        TrustError::KeyMismatch {
            name,
            expected,
            got,
        } => {
            assert_eq!(name, "Study-PC");
            assert!(expected.starts_with("AAAA1111"));
            assert!(got.starts_with("EEEE1111"));
        }
        other => panic!("expected KeyMismatch, got {other:?}"),
    }
    // The store must be unchanged after the rejected sighting.
    let store = TrustStore::load(&dir).unwrap();
    assert_eq!(store.peers().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrupted_key_file_is_typed_error_and_is_not_regenerated() {
    let dir = tmpdir("corrupt-key");
    let _ = DeviceIdentity::load_or_create(&dir).unwrap();
    std::fs::write(dir.join("device.key"), [0u8; 10]).unwrap(); // truncated

    let err = DeviceIdentity::load_or_create(&dir).unwrap_err();
    assert!(matches!(err, IdentityError::CorruptedKey), "got {err:?}");
    // Not overwritten: still our 10 junk bytes, not a fresh key.
    assert_eq!(std::fs::read(dir.join("device.key")).unwrap().len(), 10);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn corrupted_meta_file_is_typed_error() {
    let dir = tmpdir("corrupt-meta");
    let _ = DeviceIdentity::load_or_create(&dir).unwrap();
    std::fs::write(dir.join("device.json"), "{ not json").unwrap();

    let err = DeviceIdentity::load_or_create(&dir).unwrap_err();
    assert!(
        matches!(err, IdentityError::CorruptedMetadata(_)),
        "got {err:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn key_and_meta_from_different_identities_refuse_to_mix() {
    let dir_a = tmpdir("mix-a");
    let dir_b = tmpdir("mix-b");
    let a = DeviceIdentity::load_or_create(&dir_a).unwrap();
    let b = DeviceIdentity::load_or_create(&dir_b).unwrap();
    if a.fingerprint() == b.fingerprint() {
        return; // astronomically unlikely; skip rather than flake
    }
    // B's key with A's metadata.
    std::fs::copy(dir_b.join("device.key"), dir_a.join("device.key")).unwrap();
    let err = DeviceIdentity::load_or_create(&dir_a).unwrap_err();
    assert!(matches!(err, IdentityError::KeyMetaMismatch), "got {err:?}");
    let _ = std::fs::remove_dir_all(&dir_a);
    let _ = std::fs::remove_dir_all(&dir_b);
}
