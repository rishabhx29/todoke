//! Todoke transfer engine.
//!
//! `todoke-core` implements the headless transfer engine behind the Todoke app:
//! LAN device discovery, identity and trust, encrypted QUIC transport,
//! content-defined chunking, manifests, parallel multi-file transfers,
//! resume across reconnects, and verification receipts.
//!
//! This crate is a library first: the CLI (`todoke-cli`) and the future GUI
//! both consume its public async API.

/// Engine crate version, mirroring the Cargo manifest version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub mod chunk;
pub mod discovery;
pub mod identity;
pub mod manifest;
pub mod protocol;
pub mod transfer;
pub mod transport;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_semver() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert!(parts.len() >= 2, "version should be semver-ish: {VERSION}");
    }
}
