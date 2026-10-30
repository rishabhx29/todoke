//! `todoke` — the command-line harness around the Todoke transfer engine.
//!
//! Exit codes: 0 success · 1 runtime failure · 2 usage error · 130 cancelled.

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    time::Duration,
};

use base64::Engine as _;
use clap::{Parser, Subcommand};
use todoke_core::transport::CertificateDer;
use tokio::sync::mpsc;

mod bench;

use todoke_core::{
    discovery::{self, DeviceRecord},
    identity::DeviceIdentity,
    manifest::Receipt,
    transfer::{self, ReceiveOptions, SendOptions, SenderInfo, TransferError, TransferEvent},
    transport,
};

#[derive(Parser, Debug)]
#[command(
    name = "todoke",
    version,
    about = "Files that move. Fast, verifiable, restartable transfers between your devices."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run in receive mode: accept transfers from other devices into a target folder
    Serve {
        /// Where received files are finalized
        #[arg(long, short)]
        target: PathBuf,
        /// Staging root for in-progress transfers (default: <data>/staging)
        #[arg(long)]
        staging: Option<PathBuf>,
        /// Device name shown to other devices (default: generated)
        #[arg(long)]
        name: Option<String>,
        /// Max concurrent data streams per transfer
        #[arg(long, default_value_t = 4)]
        parallelism: usize,
        /// Advertise on this address/interface (default: all interfaces;
        /// pass 127.0.0.1 for same-machine testing — Windows Firewall blocks
        /// mDNS from unsigned binaries on real interfaces)
        #[arg(long, default_value = "0.0.0.0")]
        advertise_ip: String,
        /// Emit machine-readable JSON lines instead of human output
        #[arg(long)]
        json: bool,
    },
    /// Send files or directories to a nearby device
    Send {
        /// Files and/or directories to send
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Destination device: exact name or fingerprint (first 8+ chars).
        /// Mutually exclusive with --addr (direct mode).
        #[arg(long, short = 't')]
        to: Option<String>,
        /// Direct mode: dial this ip:port without discovery (useful for
        /// scripting; pair with --fingerprint and --cert-b64)
        #[arg(long)]
        addr: Option<String>,
        /// Direct mode: the destination's identity fingerprint
        #[arg(long)]
        fingerprint: Option<String>,
        /// Direct mode: the destination's advertised certificate (base64 DER)
        #[arg(long)]
        cert_b64: Option<String>,
        /// Max concurrent data streams
        #[arg(long, default_value_t = 4)]
        parallelism: usize,
        /// Seconds to spend discovering the destination
        #[arg(long, default_value_t = 6)]
        discovery_secs: u64,
        /// Emit machine-readable JSON instead of human output
        #[arg(long)]
        json: bool,
    },
    /// List Todoke devices discovered on the local network
    Devices {
        /// Seconds to spend browsing
        #[arg(long, short, default_value_t = 4)]
        timeout: u64,
        /// Emit machine-readable JSON instead of a table
        #[arg(long)]
        json: bool,
    },
    /// Run benchmark scenarios against the engine
    Bench {
        /// Scenario: big-file | many-small | resume (default: all)
        #[arg(long)]
        scenario: Option<String>,
        /// Write results as JSON to this file
        #[arg(long)]
        output: Option<PathBuf>,
    },
}

fn data_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("TODOKE_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    dirs::data_dir()
        .map(|d| d.join("todoke"))
        .ok_or_else(|| "could not locate the app-data directory".to_string())
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn load_identity() -> DeviceIdentity {
    let dir = data_dir().unwrap_or_else(|e| fail(&e));
    DeviceIdentity::load_or_create(&dir).unwrap_or_else(|e| fail(&format!("identity: {e}")))
}

fn decode_cert(record: &DeviceRecord) -> CertificateDer<'static> {
    let der = base64::engine::general_purpose::STANDARD
        .decode(&record.cert_b64)
        .unwrap_or_else(|e| fail(&format!("device advertised a corrupt certificate: {e}")));
    CertificateDer::from(der)
}

/// Find the destination by exact name or fingerprint prefix.
fn find_device(name_or_fp: &str, secs: u64) -> Result<DeviceRecord, String> {
    let needle = name_or_fp.to_lowercase();
    let records = discovery::browse_blocking(std::time::Instant::now() + Duration::from_secs(secs))
        .map_err(|e| format!("discovery failed: {e}"))?;
    let matches: Vec<&DeviceRecord> = records
        .iter()
        .filter(|r| {
            r.name.to_lowercase() == needle || r.fingerprint.to_lowercase().starts_with(&needle)
        })
        .collect();
    match matches.len() {
        1 => Ok(matches[0].clone()),
        0 => Err(format!(
            "no device matching '{name_or_fp}' found on this network — run `todoke devices` to see what's out there (both devices must run Todoke and share the network)"
        )),
        n => Err(format!(
            "{n} devices match '{name_or_fp}' — use a longer fingerprint prefix to disambiguate"
        )),
    }
}

fn short_fp(fp: &str) -> String {
    fp.chars().take(9).collect()
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Devices { timeout, json } => devices(timeout, json),
        Command::Send {
            paths,
            to,
            addr,
            fingerprint,
            cert_b64,
            parallelism,
            discovery_secs,
            json,
        } => send(
            paths,
            to,
            addr,
            fingerprint,
            cert_b64,
            parallelism,
            discovery_secs,
            json,
        ),
        Command::Serve {
            target,
            staging,
            name,
            parallelism,
            advertise_ip,
            json,
        } => serve(target, staging, name, parallelism, advertise_ip, json),
        Command::Bench { scenario, output } => {
            // Small default so a smoke run is seconds; real published runs
            // pass --size-mb 256+.
            let size_mb: u64 = std::env::var("TODOKE_BENCH_SIZE_MB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(64);
            match bench::run(scenario.as_deref(), size_mb) {
                Ok(report) => {
                    if let Err(e) = bench::write_output(&report, output.as_deref()) {
                        eprintln!("error: writing results: {e}");
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

fn devices(timeout: u64, json: bool) {
    let records =
        discovery::browse_blocking(std::time::Instant::now() + Duration::from_secs(timeout))
            .unwrap_or_else(|e| fail(&format!("discovery failed: {e}")));
    if json {
        let list: Vec<serde_json::Value> = records
            .iter()
            .map(|r| {
                serde_json::json!({
                    "name": r.name,
                    "fingerprint": r.fingerprint,
                    "addr": r.addr.to_string(),
                    "port": r.port,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&list).unwrap());
        return;
    }
    if records.is_empty() {
        println!(
            "no Todoke devices found — is another device running `todoke serve` on this network?"
        );
        return;
    }
    println!(
        "{:<22} {:<12} {}",
        "NAME",
        "FINGERPRINT",
        concat!("ADD", "R")
    );
    for r in &records {
        println!(
            "{:<22} {:<12} {}:{}",
            r.name,
            short_fp(&r.fingerprint),
            r.addr,
            r.port
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn send(
    paths: Vec<PathBuf>,
    to: Option<String>,
    addr: Option<String>,
    fingerprint: Option<String>,
    cert_b64: Option<String>,
    parallelism: usize,
    discovery_secs: u64,
    json: bool,
) {
    for p in &paths {
        if !p.exists() {
            fail(&format!("path not found: {}", p.display()));
        }
    }
    let (record, dest_name, dest_fp) = if let Some(name) = &to {
        let record = find_device(name, discovery_secs).unwrap_or_else(|e| fail(&e));
        let name = record.name.clone();
        let fp = record.fingerprint.clone();
        (record, name, fp)
    } else {
        let (Some(addr), Some(fp), Some(cert)) = (&addr, &fingerprint, &cert_b64) else {
            fail("specify either --to <device> or all of --addr, --fingerprint and --cert-b64");
        };
        let parsed: std::net::SocketAddr = addr
            .parse()
            .unwrap_or_else(|e| fail(&format!("invalid --addr: {e}")));
        let record = DeviceRecord {
            name: addr.clone(),
            fingerprint: fp.clone(),
            cert_b64: cert.clone(),
            port: parsed.port(),
            addr: match parsed.ip() {
                std::net::IpAddr::V4(v4) => v4,
                _other => fail("direct mode requires an IPv4 address (v1 limitation)"),
            },
        };
        (record, addr.clone(), fp.clone())
    };
    let cert = decode_cert(&record);
    let identity = load_identity();

    let runtime = tokio::runtime::Runtime::new().unwrap_or_else(|e| fail(&format!("runtime: {e}")));
    let result = runtime.block_on(async move {
        let peer = std::net::SocketAddr::from((record.addr, record.port));
        eprintln!("DBG send: dialing {peer} (fp {})", record.fingerprint);
        let (_endpoint, conn) = transport::dial(peer, &record.fingerprint, cert).await?;
        eprintln!("DBG send: connected");
        let sender = SenderInfo {
            name: identity.name().to_owned(),
            fingerprint: identity.fingerprint(),
        };

        if json {
            let opts = SendOptions {
                parallelism,
                progress: None,
            };
            return transfer::send_with(&conn, &paths, &sender, &opts).await;
        }

        let (event_tx, mut event_rx) = mpsc::unbounded_channel();
        let opts = SendOptions {
            parallelism,
            progress: Some(event_tx),
        };
        let bar = indicatif::ProgressBar::new_spinner();
        bar.set_style(
            indicatif::ProgressStyle::with_template(
                "{spinner:.green} {msg} [{elapsed_precise}] {bytes} ({bytes_per_sec})",
            )
            .unwrap_or_else(|_| indicatif::ProgressStyle::default_spinner()),
        );
        let send_fut = transfer::send_with(&conn, &paths, &sender, &opts);
        tokio::pin!(send_fut);
        let send = tokio::select! {
            r = &mut send_fut => Some(r),
            _ = async {
                loop {
                    match event_rx.recv().await {
                        Some(TransferEvent::FileStarted { path, size }) => {
                            bar.set_message(format!("{path} ({})…", indicatif::HumanBytes(size)));
                        }
                        Some(TransferEvent::ChunkSent { bytes, .. }) => {
                            bar.inc(bytes);
                        }
                        Some(TransferEvent::FileSent { path }) => {
                            bar.set_message(format!("{path} sent — verifying…"));
                        }
                        None => break,
                    }
                }
            } => None,
            _ = tokio::signal::ctrl_c() => None,
        };
        match send {
            Some(r) => {
                bar.finish_and_clear();
                r
            }
            None => {
                bar.finish_and_clear();
                eprintln!("cancelled — rerun the same command to resume where it left off");
                std::process::exit(130);
            }
        }
    });

    match result {
        Ok(report) => {
            if json {
                let out = serde_json::json!({
                    "ok": true,
                    "to": { "name": dest_name, "fingerprint": dest_fp },
                    "manifest_hash": report.manifest.hash,
                    "files": report.receipt.files.iter().map(|f| serde_json::json!({
                        "path": f.path, "size": f.size, "hash": f.hash, "chunks": f.chunks,
                    })).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&out).unwrap());
            } else {
                println!(
                    "✓ {} file(s), {} verified end-to-end → {}",
                    report.receipt.files.len(),
                    indicatif::HumanBytes(report.receipt.files.iter().map(|f| f.size).sum()),
                    dest_name
                );
            }
        }
        Err(e) => {
            if json {
                let out = serde_json::json!({ "ok": false, "error": e.to_string() });
                println!("{out}");
            }
            fail(&format!(
                "transfer failed: {e} (files received before the failure are kept; rerun to resume)"
            ));
        }
    }
}

fn serve(
    target: PathBuf,
    staging: Option<PathBuf>,
    name: Option<String>,
    parallelism: usize,
    advertise_ip: String,
    json: bool,
) {
    let staging =
        staging.unwrap_or_else(|| data_dir().unwrap_or_else(|e| fail(&e)).join("staging"));
    let mut identity = load_identity();
    if let Some(name) = &name {
        identity
            .set_name(name.clone())
            .unwrap_or_else(|e| fail(&format!("rename: {e}")));
    }
    ensure_target(&target);
    let runtime = tokio::runtime::Runtime::new().unwrap_or_else(|e| fail(&format!("runtime: {e}")));
    runtime.block_on(async move {
        let server_identity = transport::server_identity(&identity)
            .unwrap_or_else(|e| fail(&format!("identity cert: {e}")));
        let fingerprint = identity.fingerprint();
        let (endpoint, addr) = transport::serve(&server_identity, "0.0.0.0:0".parse().unwrap())
            .await
            .unwrap_or_else(|e| fail(&format!("cannot bind: {e}")));

        let advertiser = discovery::Advertiser::advertise_on(
            &identity,
            &server_identity,
            addr.port(),
            &advertise_ip,
        )
        .unwrap_or_else(|e| {
                fail(&format!(
                    "cannot advertise on this network: {e} (Windows Firewall blocks mDNS for unsigned binaries — run once as admin: netsh advfirewall firewall add rule name=\"Todoke\" dir=in action=allow protocol=UDP localport=5353)"
                ))
            });

        if json {
            // A wildcard bind is not dialable — report the loopback form so
            // consumers of this line can connect directly.
            let dialable = if addr.ip().is_unspecified() {
                std::net::SocketAddr::new(
                    std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
                    addr.port(),
                )
            } else {
                addr
            };
            println!(
                "{}",
                serde_json::json!({
                    "serving": true, "name": identity.name(), "fingerprint": fingerprint,
                    "addr": dialable.to_string(), "target": target.display().to_string(),
                    "cert_b64": base64::engine::general_purpose::STANDARD
                        .encode(server_identity.cert_der()),
                })
            );
        } else {
            let _ = std::io::stdout().flush();
            println!(
                "Todoke is receiving as “{}” ({}) on {}",
                identity.name(),
                fingerprint,
                addr
            );
            println!("Files land in {} — Ctrl-C to stop", target.display());
        }

        let mut opts = ReceiveOptions::new(target, staging);
        opts.parallelism = parallelism;
        let shutdown = tokio::signal::ctrl_c();
        tokio::pin!(shutdown);

        loop {
            let incoming = tokio::select! {
                inc = endpoint.accept() => inc,
                _ = &mut shutdown => {
                    if json {
                        println!("{{\"serving\": false}}");
                    } else {
                        println!("\nbye");
                    }
                    drop(advertiser.stop());
                    std::process::exit(0);
                }
            };
            let Some(incoming) = incoming else { continue };
            let Ok(conn) = incoming.await else { continue };
            let opts = opts.clone();
            tokio::spawn(async move {
                match transfer::receive(&conn, &opts).await {
                    Ok(receipt) => print_receipt(&receipt, json),
                    Err(TransferError::Cancelled(r)) => eprintln!("transfer cancelled by peer: {r}"),
                    Err(e) => eprintln!("transfer failed: {e} (staging kept — it will resume)"),
                }
            });
        }
    });
}

fn print_receipt(receipt: &Receipt, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::json!({
                "received": true,
                "manifest_hash": receipt.manifest_hash,
                "files": receipt.files.iter().map(|f| serde_json::json!({
                    "path": f.path, "size": f.size, "hash": f.hash, "chunks": f.chunks,
                })).collect::<Vec<_>>(),
            })
        );
        return;
    }
    let bytes: u64 = receipt.files.iter().map(|f| f.size).sum();
    println!(
        "✓ received {} file(s), {} — verified intact",
        receipt.files.len(),
        indicatif::HumanBytes(bytes)
    );
}

/// Fail fast if the target cannot be created — before advertising, so the
/// device never shows up on the network while unable to receive.
fn ensure_target(target: &Path) {
    if let Err(e) = std::fs::create_dir_all(target) {
        fail(&format!("target dir {}: {e}", target.display()));
    }
}
