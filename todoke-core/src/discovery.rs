//! LAN device discovery over mDNS (mdns-sd).
//!
//! Every Todoke device advertises `_todoke._udp.local.` with TXT records
//! carrying its name, identity fingerprint, and certificate chunks (the
//! certificate is what the transport layer pins; the fingerprint is what
//! the trust store checks). Browsing yields typed [`DeviceRecord`]s — no
//! raw mDNS structures leak — deduplicated by fingerprint, with malformed
//! advertisements rejected rather than surfaced.

use std::{
    collections::HashMap,
    net::Ipv4Addr,
    time::{Duration, Instant},
};

use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

/// mDNS service type Todoke devices advertise.
pub const SERVICE_TYPE: &str = "_todoke._udp.local.";

/// Longest certificate base64 chunk that fits mdns-sd's 255-byte TXT limit
/// (`len(key + value) < u8::MAX`), with headroom for the key name.
const CERT_CHUNK_LEN: usize = 200;
const MAX_CERT_CHUNKS: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum DiscoveryError {
    #[error("mDNS daemon error: {0}")]
    Daemon(String),
    #[error("malformed advertisement: {0}")]
    Malformed(String),
    #[error("device name is empty")]
    EmptyName,
    #[error("invalid fingerprint format: {0}")]
    BadFingerprint(String),
    #[error("missing or malformed certificate chunks")]
    BadCertificate,
}

/// A nearby Todoke device, fully typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRecord {
    pub name: String,
    pub fingerprint: String,
    /// Certificate DER (base64) to pin at connect time.
    pub cert_b64: String,
    pub port: u16,
    pub addr: Ipv4Addr,
}

impl DeviceRecord {
    /// Structural validation for records built from wire data.
    pub fn validate(&self) -> Result<(), DiscoveryError> {
        if self.name.is_empty() {
            return Err(DiscoveryError::EmptyName);
        }
        if !is_valid_fingerprint(&self.fingerprint) {
            return Err(DiscoveryError::BadFingerprint(self.fingerprint.clone()));
        }
        if self.cert_b64.is_empty() {
            return Err(DiscoveryError::BadCertificate);
        }
        Ok(())
    }
}

fn is_valid_fingerprint(fp: &str) -> bool {
    let bytes = fp.as_bytes();
    if bytes.len() != 35 {
        return false;
    }
    for (i, b) in bytes.iter().enumerate() {
        let is_hex = b.is_ascii_digit() || (b'a'..=b'f').contains(b) || (b'A'..=b'F').contains(b);
        let is_dash = *b == b'-';
        let want_dash = i % 9 == 8;
        if want_dash != is_dash {
            return false;
        }
        if !want_dash && !is_hex {
            return false;
        }
    }
    true
}

/// A live advertisement. Dropping it unregisters the service (best effort);
/// call [`Advertiser::stop`] for a confirmed shutdown.
pub struct Advertiser {
    daemon: ServiceDaemon,
    fullname: String,
    service_type: String,
}

impl Advertiser {
    /// Advertise this device on all IPv4 interfaces (production path).
    pub fn advertise(
        identity: &crate::identity::DeviceIdentity,
        server_identity: &crate::transport::ServerIdentity,
        port: u16,
    ) -> Result<Self, DiscoveryError> {
        Self::advertise_on(identity, server_identity, port, "0.0.0.0")
    }

    /// Advertise on a specific address/interface — the hermetic test path
    /// uses `127.0.0.1` (Windows Firewall silently drops multicast from
    /// unsigned binaries on real interfaces; loopback is unaffected).
    pub fn advertise_on(
        identity: &crate::identity::DeviceIdentity,
        server_identity: &crate::transport::ServerIdentity,
        port: u16,
        ip: &str,
    ) -> Result<Self, DiscoveryError> {
        let daemon = ServiceDaemon::new().map_err(|e| DiscoveryError::Daemon(e.to_string()))?;
        // IPv4 only for v1 (QUIC server binds v4); avoids dual-stack noise.
        daemon
            .disable_interface(mdns_sd::IfKind::IPv6)
            .map_err(|e| DiscoveryError::Daemon(e.to_string()))?;

        let props = txt_properties(
            identity.name(),
            &identity.fingerprint(),
            server_identity.cert_der(),
        )?;

        let host = format!(
            "todoke-{}.local.",
            &identity.fingerprint().to_lowercase().replace('-', "")[..12]
        );
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            identity.name(),
            &host,
            ip,
            port,
            props.as_slice(),
        )
        .map_err(|e| DiscoveryError::Daemon(e.to_string()))?;
        daemon
            .register(info)
            .map_err(|e| DiscoveryError::Daemon(e.to_string()))?;

        Ok(Self {
            fullname: format!("{}.{SERVICE_TYPE}", identity.name()),
            daemon,
            service_type: SERVICE_TYPE.to_owned(),
        })
    }

    /// Unregister and stop browsing, consuming the advertiser.
    pub fn stop(self) -> Result<(), DiscoveryError> {
        let _ = self
            .daemon
            .unregister(&self.fullname)
            .map_err(|e| DiscoveryError::Daemon(e.to_string()));
        let _ = self.daemon.stop_browse(&self.service_type);
        Ok(())
    }
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        let _ = self.daemon.unregister(&self.fullname);
        let _ = self.daemon.stop_browse(&self.service_type);
    }
}

/// TXT records for an advertisement. Splits the certificate base64 into
/// `cert0..certN` chunks under the TXT length limit.
fn txt_properties(
    name: &str,
    fingerprint: &str,
    cert_der: &[u8],
) -> Result<Vec<(String, String)>, DiscoveryError> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD.encode(cert_der);
    let mut props = vec![
        ("ver".to_owned(), "1".to_owned()),
        ("name".to_owned(), name.to_owned()),
        ("fp".to_owned(), fingerprint.to_owned()),
    ];
    let chunks: Vec<&str> = if b64.is_empty() {
        Vec::new()
    } else {
        b64.as_bytes()
            .chunks(CERT_CHUNK_LEN)
            .map(|c| std::str::from_utf8(c).expect("base64 is utf8"))
            .collect()
    };
    if chunks.len() > MAX_CERT_CHUNKS {
        return Err(DiscoveryError::BadCertificate);
    }
    for (i, chunk) in chunks.iter().enumerate() {
        props.push((format!("cert{i}"), (*chunk).to_owned()));
    }
    Ok(props)
}

/// Browse the LAN for Todoke devices until `timeout` elapses (wall clock),
/// returning deduplicated, validated records in first-seen order.
///
/// The blocking mDNS receive loop runs on a dedicated thread.
pub async fn browse(timeout: Duration) -> Result<Vec<DeviceRecord>, DiscoveryError> {
    let deadline = Instant::now() + timeout;
    tokio::task::spawn_blocking(move || browse_blocking(deadline))
        .await
        .map_err(|e| DiscoveryError::Daemon(e.to_string()))?
}

/// Blocking browse used by [`browse`] and available to the CLI directly.
pub fn browse_blocking(deadline: Instant) -> Result<Vec<DeviceRecord>, DiscoveryError> {
    let daemon = ServiceDaemon::new().map_err(|e| DiscoveryError::Daemon(e.to_string()))?;
    let _ = daemon.disable_interface(mdns_sd::IfKind::IPv6);
    let receiver = daemon
        .browse(SERVICE_TYPE)
        .map_err(|e| DiscoveryError::Daemon(e.to_string()))?;

    // fullname -> record (dedupe by fingerprint happens at the end; duplicates
    // of the same fullname carry the same fingerprint).
    let mut by_fullname: HashMap<String, DeviceRecord> = HashMap::new();

    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        let remaining = deadline - now;
        match receiver.recv_timeout(remaining.min(Duration::from_millis(250))) {
            Ok(ServiceEvent::ServiceResolved(resolved)) => {
                if let Some(record) = record_from_resolved(&resolved) {
                    by_fullname.insert(resolved.get_fullname().to_owned(), record);
                }
            }
            Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                by_fullname.remove(&fullname);
            }
            Ok(_) => {}
            Err(mdns_sd::RecvTimeoutError::Timeout) => continue,
            Err(mdns_sd::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = daemon.stop_browse(SERVICE_TYPE);

    // Dedupe by fingerprint, first seen wins, preserving insertion order.
    let mut seen = std::collections::HashSet::new();
    Ok(by_fullname
        .into_values()
        .filter(|r| seen.insert(r.fingerprint.clone()))
        .collect())
}

fn record_from_resolved(resolved: &mdns_sd::ResolvedService) -> Option<DeviceRecord> {
    let txt = resolved.get_property("fp")?.val_str().to_owned();
    let name = resolved
        .get_property("name")
        .map(|p| p.val_str().to_owned())
        .unwrap_or_default();

    let mut chunks: Vec<String> = Vec::new();
    for i in 0..MAX_CERT_CHUNKS {
        match resolved.get_property(&format!("cert{i}")) {
            Some(p) => chunks.push(p.val_str().to_owned()),
            None => break,
        }
    }
    let cert_b64 = chunks.concat();

    let addr = *resolved.get_addresses_v4().iter().next()?;
    let record = DeviceRecord {
        name,
        fingerprint: txt,
        cert_b64,
        port: resolved.port,
        addr,
    };
    match record.validate() {
        Ok(()) => Some(record),
        // Malformed advertisements are dropped, never surfaced.
        Err(_) => None,
    }
}
