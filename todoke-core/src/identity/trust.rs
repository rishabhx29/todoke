//! Trust-on-first-use store for peer devices.
//!
//! A peer is remembered by name with the key fingerprint seen at first
//! contact. A later sighting of the same name under a *different*
//! fingerprint is an impersonation attempt (or a reinstall) and surfaces as
//! [`TrustError::KeyMismatch`] — it is never silently overwritten.

use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

const STORE_FILE: &str = "trusted_peers.json";

#[derive(Debug, thiserror::Error)]
pub enum TrustError {
    #[error("trust store I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("trust store file is corrupt: {0}")]
    Corrupted(#[from] serde_json::Error),
    #[error(
        "device '{name}' is already trusted with a different key (expected {expected}, saw {got})"
    )]
    KeyMismatch {
        name: String,
        expected: String,
        got: String,
    },
}

/// A peer remembered by the trust store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub name: String,
    pub fingerprint: String,
    pub first_seen_unix: u64,
}

/// The set of trusted peers, persisted as JSON in the app dir.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TrustStore {
    peers: Vec<TrustedPeer>,
}

impl TrustStore {
    /// Load the store; a missing file yields an empty store (nothing trusted yet).
    pub fn load(dir: &Path) -> Result<Self, TrustError> {
        let path = dir.join(STORE_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
    }

    /// All trusted peers, in first-seen order.
    pub fn peers(&self) -> &[TrustedPeer] {
        &self.peers
    }

    /// Find a peer by its key fingerprint.
    pub fn lookup(&self, fingerprint: &str) -> Option<&TrustedPeer> {
        self.peers.iter().find(|p| p.fingerprint == fingerprint)
    }

    fn find_by_name(&self, name: &str) -> Option<&TrustedPeer> {
        self.peers.iter().find(|p| p.name == name)
    }

    /// Verify a sighting; record it if the peer is unknown (trust on first use).
    ///
    /// Errors with [`TrustError::KeyMismatch`] when a known name presents a
    /// different key than the one recorded — the caller must treat that as a
    /// possible impersonation and abort the connection.
    pub fn verify_or_record(
        &mut self,
        dir: &Path,
        name: &str,
        fingerprint: &str,
    ) -> Result<(), TrustError> {
        match self.find_by_name(name) {
            Some(peer) if peer.fingerprint == fingerprint => Ok(()),
            Some(peer) => Err(TrustError::KeyMismatch {
                name: name.to_owned(),
                expected: peer.fingerprint.clone(),
                got: fingerprint.to_owned(),
            }),
            None => {
                self.peers.push(TrustedPeer {
                    name: name.to_owned(),
                    fingerprint: fingerprint.to_owned(),
                    first_seen_unix: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_secs()),
                });
                self.save(dir)
            }
        }
    }

    /// Explicitly record a peer (same rules as [`Self::verify_or_record`]).
    pub fn record_peer(
        &mut self,
        dir: &Path,
        name: &str,
        fingerprint: &str,
    ) -> Result<(), TrustError> {
        self.verify_or_record(dir, name, fingerprint)
    }

    fn save(&self, dir: &Path) -> Result<(), TrustError> {
        fs::write(dir.join(STORE_FILE), serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}
