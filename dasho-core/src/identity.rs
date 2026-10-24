//! Device identity: an Ed25519 keypair with a human-friendly name, persisted
//! in the Dasho app-data directory, plus the trust-on-first-use peer store.
//!
//! Layout inside the app dir:
//! - `device.key` — 32-byte raw Ed25519 seed (0600 on unix)
//! - `device.json` — name, creation time, and a key-check linking meta to key
//! - `trusted_peers.json` — TOFU store of seen peers
//!
//! Corruption of any identity file is a typed error; we never silently
//! regenerate over existing key material.

mod trust;

pub use trust::{TrustError, TrustStore, TrustedPeer};

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use getrandom::{SysRng, rand_core::UnwrapErr};
use serde::{Deserialize, Serialize};

const KEY_FILE: &str = "device.key";
const META_FILE: &str = "device.json";
/// Bytes of the public key used for the display fingerprint (128 bits).
const FINGERPRINT_BYTES: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("identity I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("identity metadata file is corrupt: {0}")]
    CorruptedMetadata(#[from] serde_json::Error),
    #[error("identity key file is corrupt or truncated (expected 32 bytes)")]
    CorruptedKey,
    #[error("identity metadata does not match the stored key — refusing to mix them")]
    KeyMetaMismatch,
}

#[derive(Debug, Serialize, Deserialize)]
struct IdentityMeta {
    name: String,
    created_at_unix: u64,
    /// First 8 hex chars of the public key; ties the JSON to the key file.
    key_check: String,
}

/// A Dasho device identity.
#[derive(Debug)]
pub struct DeviceIdentity {
    dir: PathBuf,
    key: SigningKey,
    meta: IdentityMeta,
}

impl DeviceIdentity {
    /// Load the identity from `dir`, generating and persisting a fresh one
    /// only when no key file exists. Existing-but-corrupt files are errors.
    pub fn load_or_create(dir: &Path) -> Result<Self, IdentityError> {
        fs::create_dir_all(dir)?;
        let key_path = dir.join(KEY_FILE);
        let meta_path = dir.join(META_FILE);

        let key = if key_path.exists() {
            let bytes = fs::read(&key_path)?;
            let bytes: [u8; 32] = bytes.try_into().map_err(|_| IdentityError::CorruptedKey)?;
            SigningKey::from_bytes(&bytes)
        } else {
            let key = SigningKey::generate(&mut UnwrapErr(SysRng));
            fs::write(&key_path, key.to_bytes())?;
            restrict_permissions(&key_path);
            key
        };

        let check = key_check(&key);
        let meta = if meta_path.exists() {
            let meta: IdentityMeta = serde_json::from_str(&fs::read_to_string(&meta_path)?)?;
            if meta.key_check != check {
                return Err(IdentityError::KeyMetaMismatch);
            }
            meta
        } else {
            let meta = IdentityMeta {
                name: format!("dasho-{}", check[..4].to_lowercase()),
                created_at_unix: unix_now(),
                key_check: check,
            };
            write_meta(&meta_path, &meta)?;
            meta
        };

        Ok(Self {
            dir: dir.to_path_buf(),
            key,
            meta,
        })
    }

    /// The device's display name (editable; persisted separately from the key).
    pub fn name(&self) -> &str {
        &self.meta.name
    }

    /// Rename the device. Persists metadata only — the key file is untouched.
    pub fn set_name(&mut self, name: impl Into<String>) -> Result<(), IdentityError> {
        self.meta.name = name.into();
        write_meta(&self.dir.join(META_FILE), &self.meta)
    }

    /// The Ed25519 public key other devices verify against.
    pub fn public_key(&self) -> VerifyingKey {
        self.key.verifying_key()
    }

    /// The raw 32-byte Ed25519 seed (used to derive the TLS certificate key).
    pub fn seed_bytes(&self) -> [u8; 32] {
        self.key.to_bytes()
    }

    /// Stable display fingerprint: first 128 bits of the public key as
    /// upper-case hex in `XXXXXXXX-XXXXXXXX-XXXXXXXX-XXXXXXXX` form.
    pub fn fingerprint(&self) -> String {
        format_fingerprint(&self.key.verifying_key().to_bytes())
    }

    /// Sign a message with the device key.
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.key.sign(message)
    }

    /// Verify a peer's signature (helper; `VerifyingKey::verify` also works).
    pub fn verify(peer: &VerifyingKey, message: &[u8], signature: &Signature) -> bool {
        peer.verify(message, signature).is_ok()
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn key_check(key: &SigningKey) -> String {
    hex(&key.verifying_key().to_bytes()[..4])
}

fn write_meta(path: &Path, meta: &IdentityMeta) -> Result<(), IdentityError> {
    fs::write(path, serde_json::to_string_pretty(meta)?)?;
    Ok(())
}

fn format_fingerprint(pubkey: &[u8; 32]) -> String {
    let h = hex(&pubkey[..FINGERPRINT_BYTES]).to_uppercase();
    (0..4)
        .map(|i| &h[i * 8..(i + 1) * 8])
        .collect::<Vec<_>>()
        .join("-")
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

#[cfg(unix)]
fn restrict_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path) {
    // Windows ACLs: the file lives in the user's own app-data profile dir.
}
