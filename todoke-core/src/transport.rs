//! QUIC transport: identity-bound certificates, endpoint configuration, and
//! the transport seam the protocol layer programs against.
//!
//! Security model (deterministic identity certificates + TOFU pinning):
//! - A device's TLS certificate is a pure function of its Ed25519 identity
//!   key: fixed validity window, serial derived from the public key, SAN
//!   `todoke-<fp8>.todoke.internal`, self-signed with the device key itself.
//! - The certificate travels out-of-band (discovery TXT chunks — wired in
//!   ticket 04; callers here just pass the DER bytes).
//! - A client pins the received certificate as its ONLY trust anchor after
//!   the advertised fingerprint matches its trust store, then connects with
//!   `server_name` = the SAN. rustls then enforces: chain-to-pinned-root,
//!   SAN match, TLS 1.3. An impostor without the peer's identity key cannot
//!   present a certificate that both matches the pinned fingerprint and
//!   chains to itself as the pinned root.
//! - No `dangerous_configuration`, no custom certificate verifiers: only
//!   public, audited rustls paths.

use std::{fmt, net::SocketAddr, sync::Arc, time::Duration};

use quinn::{RecvStream, SendStream};
pub use rustls::pki_types::CertificateDer;
use rustls::pki_types::PrivatePkcs8KeyDer;

use crate::identity::DeviceIdentity;

/// How long a connection handshake may take before we give up (quinn
/// otherwise retries forever against an unreachable/firewalled peer).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Fixed validity window for identity certificates. Dates are fixed (not
/// clock-derived) so the certificate is fully determined by the key.
const CERT_VALID_FROM: (i32, u8, u8) = (2026, 1, 1);
const CERT_VALID_TO: (i32, u8, u8) = (2099, 1, 1);

/// DNS suffix for device SANs; full name: `todoke-<fp8>.todoke.internal`.
const SAN_SUFFIX: &str = "todoke.internal";

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("socket error: {0}")]
    Io(#[from] std::io::Error),
    #[error("certificate generation failed: {0}")]
    Cert(#[from] rcgen::Error),
    #[error("TLS configuration error: {0}")]
    Tls(#[from] rustls::Error),
    #[error("connection failed: {0}")]
    Connect(String),
    #[error("connection lost: {0}")]
    Connection(#[from] quinn::ConnectionError),
    #[error("stream error: {0}")]
    Stream(#[from] quinn::WriteError),
    #[error("unexpected end of stream")]
    UnexpectedEof,
}

/// The DNS name a device's certificate is issued for.
pub fn device_dns_name(fingerprint: &str) -> String {
    let compact: String = fingerprint.chars().filter(|c| *c != '-').collect();
    let head: String = compact.chars().take(16).collect::<String>().to_lowercase();
    format!("todoke-{head}.{SAN_SUFFIX}")
}

/// Build the rcgen key pair for a device identity from its raw Ed25519 seed,
/// wrapped in the standard Ed25519 PKCS#8 template.
fn pkcs8_der_for_seed(seed: &[u8; 32]) -> Vec<u8> {
    const ED25519_PKCS8_PREFIX: [u8; 16] = [
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    let mut der = Vec::with_capacity(48);
    der.extend_from_slice(&ED25519_PKCS8_PREFIX);
    der.extend_from_slice(seed);
    der
}

/// Generate the device's identity certificate: deterministic given the key.
fn device_certificate(
    key_pair: &rcgen::KeyPair,
    fingerprint: &str,
) -> Result<CertificateDer<'static>, TransportError> {
    let mut serial = [0u8; 8];
    // Serial from the public key so the same key yields the same serial.
    let pub_raw = key_pair.public_key_raw();
    serial.copy_from_slice(&pub_raw[pub_raw.len() - 8..]);

    let mut params = rcgen::CertificateParams::new(vec![device_dns_name(fingerprint)])?;
    params.not_before =
        rcgen::date_time_ymd(CERT_VALID_FROM.0, CERT_VALID_FROM.1, CERT_VALID_FROM.2);
    params.not_after = rcgen::date_time_ymd(CERT_VALID_TO.0, CERT_VALID_TO.1, CERT_VALID_TO.2);
    params.serial_number = Some(rcgen::SerialNumber::from(u64::from_be_bytes(serial)));
    params.is_ca = rcgen::IsCa::ExplicitNoCa;

    let cert = params.self_signed(key_pair)?;
    Ok(cert.der().clone())
}

/// Everything a server endpoint needs: its certificate chain and private key.
pub struct ServerIdentity {
    cert: CertificateDer<'static>,
    key_der: Vec<u8>,
}

/// Produce this device's certificate and private key for serving.
pub fn server_identity(identity: &DeviceIdentity) -> Result<ServerIdentity, TransportError> {
    let fingerprint = identity.fingerprint();
    let pkcs8 = pkcs8_der_for_seed(&identity.seed_bytes());
    let key_pair = rcgen::KeyPair::from_pkcs8_der_and_sign_algo(
        &PrivatePkcs8KeyDer::from(pkcs8.clone()),
        &rcgen::PKCS_ED25519,
    )?;
    let cert = device_certificate(&key_pair, &fingerprint)?;
    Ok(ServerIdentity {
        cert,
        key_der: pkcs8,
    })
}

impl ServerIdentity {
    /// DER bytes of the device certificate (advertised via discovery).
    pub fn cert_der(&self) -> &[u8] {
        self.cert.as_ref()
    }
}

/// Server-side endpoint configuration with the device certificate.
pub fn server_config(identity: &ServerIdentity) -> Result<quinn::ServerConfig, TransportError> {
    let mut transport = quinn::TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(10)));
    transport.max_idle_timeout(Some(
        quinn::IdleTimeout::try_from(Duration::from_secs(60)).expect("60s is a valid idle timeout"),
    ));
    transport.max_concurrent_bidi_streams(256u32.into());
    transport.max_concurrent_uni_streams(256u32.into());

    let mut config = quinn::ServerConfig::with_single_cert(
        vec![identity.cert.clone()],
        rustls::pki_types::PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.key_der.clone())),
    )?;
    config.transport_config(Arc::new(transport));
    Ok(config)
}

/// Client-side endpoint configuration pinning `pinned_cert` as the sole
/// trust anchor. Connect with `server_name = device_dns_name(fingerprint)`.
pub fn client_config(
    pinned_cert: CertificateDer<'static>,
) -> Result<quinn::ClientConfig, TransportError> {
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(pinned_cert)
        .map_err(|e| TransportError::Connect(format!("pinned certificate rejected: {e}")))?;

    let mut transport = quinn::TransportConfig::default();
    transport.keep_alive_interval(Some(Duration::from_secs(10)));
    transport.max_idle_timeout(Some(
        quinn::IdleTimeout::try_from(Duration::from_secs(60)).expect("60s is a valid idle timeout"),
    ));

    let mut config = quinn::ClientConfig::with_root_certificates(Arc::new(roots))
        .map_err(|e| TransportError::Connect(format!("client config: {e}")))?;
    config.transport_config(Arc::new(transport));
    Ok(config)
}

/// The transport seam: byte streams over an authenticated connection.
///
/// The protocol layer (ticket 05) programs against this trait, not quinn
/// types, so the future Iroh mesh transport can be added as a second
/// implementation without touching protocol code.
pub trait Transport: fmt::Debug + Send + Sync {
    /// Open a bidirectional stream.
    fn open_bi(
        &self,
    ) -> impl Future<Output = Result<(SendStream, RecvStream), TransportError>> + Send;
    /// Accept a bidirectional stream opened by the peer.
    fn accept_bi(
        &self,
    ) -> impl Future<Output = Result<(SendStream, RecvStream), TransportError>> + Send;
    /// Close the connection with a reason code.
    fn close(&self, code: u32, reason: &[u8]);
}

impl Transport for quinn::Connection {
    async fn open_bi(&self) -> Result<(SendStream, RecvStream), TransportError> {
        Ok(self.open_bi().await?)
    }

    async fn accept_bi(&self) -> Result<(SendStream, RecvStream), TransportError> {
        Ok(self.accept_bi().await?)
    }

    fn close(&self, code: u32, reason: &[u8]) {
        self.close(code.into(), reason);
    }
}

/// Bind a server endpoint advertising `identity`.
pub async fn serve(
    identity: &ServerIdentity,
    addr: SocketAddr,
) -> Result<(quinn::Endpoint, SocketAddr), TransportError> {
    let endpoint = quinn::Endpoint::server(server_config(identity)?, addr)?;
    let local = endpoint.local_addr()?;
    Ok((endpoint, local))
}

/// Connect to a peer, pinning its certificate and verifying the name.
///
/// `expected_fingerprint` must already be validated against the trust store
/// by the caller (TOFU happens at the discovery/protocol layer).
pub async fn connect(
    endpoint: &quinn::Endpoint,
    peer: SocketAddr,
    expected_fingerprint: &str,
    peer_cert: CertificateDer<'static>,
) -> Result<quinn::Connection, TransportError> {
    let config = client_config(peer_cert)?;
    let connect = endpoint
        .connect_with(config, peer, &device_dns_name(expected_fingerprint))
        .map_err(|e| TransportError::Connect(e.to_string()))?;
    let conn = connect
        .await
        .map_err(|e| TransportError::Connect(e.to_string()))?;
    Ok(conn)
}

/// Convenience: a client endpoint bound to an ephemeral local port.
///
/// NOTE: a `quinn::Connection` is only usable while its endpoint's driver
/// is alive — dropping the endpoint kills the connection ("connection
/// lost"). Callers that create a one-off endpoint must keep it alive for
/// as long as they use the connection. [`dial`] pairs the two for you.
pub fn client_endpoint() -> Result<quinn::Endpoint, TransportError> {
    Ok(quinn::Endpoint::client(SocketAddr::from((
        [0, 0, 0, 0],
        0,
    )))?)
}

/// Connect to a peer and return BOTH the endpoint and the connection.
///
/// The endpoint must be kept alive (bound) for as long as the connection
/// is used; see the note on [`client_endpoint`].
pub async fn dial(
    peer: SocketAddr,
    expected_fingerprint: &str,
    peer_cert: CertificateDer<'static>,
) -> Result<(quinn::Endpoint, quinn::Connection), TransportError> {
    let endpoint = client_endpoint()?;
    // Bounded handshake: an unreachable/firewalled peer must fail fast with
    // guidance, not retry forever (quinn connects indefinitely otherwise).
    let conn = tokio::time::timeout(
        CONNECT_TIMEOUT,
        connect(&endpoint, peer, expected_fingerprint, peer_cert),
    )
    .await
    .map_err(|_| TransportError::Connect(format!(
        "no response from {peer} within 10s — is the device running `todoke serve`, and is UDP allowed through the firewall?"
    )))??;
    Ok((endpoint, conn))
}
