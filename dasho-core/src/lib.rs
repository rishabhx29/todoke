//! Dasho transfer engine.
//!
//! `dasho-core` implements the headless transfer engine behind the Dasho app:
//! LAN device discovery, identity and trust, encrypted QUIC transport,
//! content-defined chunking, manifests, parallel multi-file transfers,
//! resume across reconnects, and verification receipts.
//!
//! This crate is a library first: the CLI (`dasho-cli`) and the future GUI
//! both consume its public async API.

/// Engine crate version, mirroring the Cargo manifest version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_matches_semver() {
        let parts: Vec<&str> = VERSION.split('.').collect();
        assert!(parts.len() >= 2, "version should be semver-ish: {VERSION}");
    }
}
