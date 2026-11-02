//! Benchmark harness: reproducible numbers for the "fastest" claim.
//!
//! All scenarios run the real engine over loopback QUIC (an in-process
//! receiver and sender, both bound to the loopback address). Loopback
//! numbers measure pure engine
//! overhead (chunking, hashing, framing, staging, finalization) with no NIC
//! in the path — they are the ceiling and the regression signal. LAN numbers
//! between two real machines come from `todoke serve` + `todoke send` with a
//! stopwatch or the same output format; the JSON schema is the same.
//!
//! Reproducibility: fixtures are deterministic (seeded PRNG), scenarios are
//! data-defined below, and every result row carries environment metadata.
//! Re-running the same scenario on the same machine should agree within
//! run-to-run noise (about ten percent); document your machine when publishing.

use std::{
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use todoke_core::transport::CertificateDer;
use todoke_core::{
    chunk::{ChunkParams, hash_reader},
    identity::DeviceIdentity,
    manifest::{Manifest, build_manifest},
    protocol::{Envelope, Message, read_message, write_data_frame, write_message},
    transfer::{self, ReceiveOptions, SenderInfo},
    transport,
};

/// Deterministic pseudo-random bytes (splitmix64) — the same seed always
/// produces the same fixture, so runs are comparable across time.
fn pseudo_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut state = seed;
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        out.extend_from_slice(&z.to_le_bytes());
    }
    out.truncate(len);
    out
}

#[derive(Debug, Serialize, Clone)]
pub struct BenchResult {
    pub scenario: String,
    pub bytes: u64,
    pub elapsed_ms: u128,
    pub throughput_mbps: f64,
    pub notes: String,
}

#[derive(Debug, Serialize)]
pub struct BenchReport {
    pub tool: String,
    pub version: String,
    pub os: String,
    pub arch: String,
    pub timestamp_unix: u64,
    pub results: Vec<BenchResult>,
}

fn env_report() -> BenchReport {
    BenchReport {
        tool: "todoke-bench".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
        timestamp_unix: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        results: Vec::new(),
    }
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn to_result(scenario: &str, bytes: u64, started: Instant, notes: &str) -> BenchResult {
    let elapsed_ms = started.elapsed().as_millis();
    let seconds = elapsed_ms as f64 / 1000.0;
    BenchResult {
        scenario: scenario.into(),
        bytes,
        elapsed_ms,
        throughput_mbps: if seconds > 0.0 {
            mb(bytes) / seconds
        } else {
            0.0
        },
        notes: notes.into(),
    }
}

/// One full transfer over loopback with an in-process receiver.
async fn loopback_transfer(
    identity: &DeviceIdentity,
    server_id: &transport::ServerIdentity,
    inputs: &[PathBuf],
    parallelism: usize,
) -> Result<u64, String> {
    let target = temp_root("bench-target");
    let staging = temp_root("bench-staging");
    let (endpoint, addr) = transport::serve(server_id, "127.0.0.1:0".parse().unwrap())
        .await
        .map_err(|e| e.to_string())?;
    let fingerprint = identity.fingerprint();
    let cert = CertificateDer::from(server_id.cert_der().to_vec());

    let opts = ReceiveOptions::new(target, staging);
    let receiver = tokio::spawn(async move {
        let incoming = endpoint
            .accept()
            .await
            .ok_or(transfer::TransferError::ConnectionLost(
                "no incoming connection".into(),
            ))?;
        let conn = incoming
            .await
            .map_err(|e| transfer::TransferError::ConnectionLost(e.to_string()))?;
        let bytes = transfer::receive(&conn, &opts)
            .await
            .map(|r| r.files.iter().map(|f| f.size).sum::<u64>());
        drop(conn);
        bytes
    });

    let (_ep, conn) = transport::dial(addr, &fingerprint, cert)
        .await
        .map_err(|e| e.to_string())?;
    let sender = SenderInfo {
        name: "bench".into(),
        fingerprint: fingerprint.clone(),
    };
    transfer::send_with(
        &conn,
        inputs,
        &sender,
        &transfer::SendOptions {
            parallelism,
            progress: None,
        },
    )
    .await
    .map_err(|e| e.to_string())?;
    let bytes = receiver
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("todoke-bench-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write_fixture(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    let mut f = fs::File::create(&path).unwrap();
    f.write_all(bytes).unwrap();
    path
}

/// Scenario: one large file — the media-transfer case (max throughput).
fn scenario_big_file(size_mb: u64) -> Result<BenchResult, String> {
    let src = temp_root("bf-src");
    let data = pseudo_bytes(0xB16, size_mb as usize * 1024 * 1024);
    let file = write_fixture(&src, "big.dat", &data);

    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let started = Instant::now();
    let bytes = runtime.block_on(async {
        let identity =
            DeviceIdentity::load_or_create(&temp_root("bf-id")).map_err(|e| e.to_string())?;
        let server_id = transport::server_identity(&identity).map_err(|e| e.to_string())?;
        loopback_transfer(&identity, &server_id, &[file], 4).await
    })?;
    let result = to_result("big-file", bytes, started, "loopback, 4 streams");
    // Integrity is part of the benchmark: the receiver whole-file-verifies
    // before finalizing, so a corrupted run already failed above.
    Ok(result)
}

/// Scenario: thousands of small files — the photo-library case (per-file
/// overhead + packing efficiency).
fn scenario_many_small() -> Result<BenchResult, String> {
    let src = temp_root("ms-src");
    let mut inputs = Vec::new();
    for i in 0..2000u32 {
        let data = pseudo_bytes(i as u64 + 1, 2048);
        inputs.push(write_fixture(&src, &format!("f{i}.bin"), &data));
    }
    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let started = Instant::now();
    let bytes = runtime.block_on(async {
        let identity =
            DeviceIdentity::load_or_create(&temp_root("ms-id")).map_err(|e| e.to_string())?;
        let server_id = transport::server_identity(&identity).map_err(|e| e.to_string())?;
        loopback_transfer(&identity, &server_id, &inputs, 4).await
    })?;
    Ok(to_result(
        "many-small",
        bytes,
        started,
        "loopback, 2000 files × 2 KiB, 4 streams",
    ))
}

/// A receiver endpoint that serves `sessions` consecutive receive sessions
/// with the same target/staging (a restarted receiver process). Each
/// session's outcome arrives on the channel as it finishes.
async fn serve_sessions(
    server_id: transport::ServerIdentity,
    opts: ReceiveOptions,
    sessions: usize,
) -> (
    std::net::SocketAddr,
    tokio::sync::mpsc::UnboundedReceiver<Result<u64, String>>,
) {
    let (endpoint, addr) = transport::serve(&server_id, "127.0.0.1:0".parse().unwrap())
        .await
        .unwrap();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        for _ in 0..sessions {
            let outcome = async {
                let incoming =
                    endpoint
                        .accept()
                        .await
                        .ok_or(transfer::TransferError::ConnectionLost(
                            "no incoming connection".into(),
                        ))?;
                let conn = incoming
                    .await
                    .map_err(|e| transfer::TransferError::ConnectionLost(e.to_string()))?;
                transfer::receive(&conn, &opts)
                    .await
                    .map(|r| r.files.iter().map(|f| f.size).sum::<u64>())
            }
            .await;
            let _ = tx.send(outcome.map_err(|e| e.to_string()));
        }
    });
    (addr, rx)
}

/// Raw session-1 sender: handshake, send the first `send_chunks` chunks of
/// the file, then break the session with a sentinel frame the receiver
/// rejects (file index past the manifest). The receiver processes frames in
/// order, so the moment it fails, exactly `send_chunks` chunks are staged —
/// the resumed session's numbers are reproducible, not timing-dependent.
async fn rogue_partial<T: transport::Transport>(
    conn: &T,
    peer_fp: &str,
    manifest: &Manifest,
    source: &Path,
    send_chunks: usize,
) {
    let (mut ctl_tx, mut ctl_rx) = conn.open_bi().await.unwrap();
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Hello {
            name: "rogue".into(),
            fingerprint: peer_fp.to_string(),
        }),
    )
    .await
    .unwrap();
    write_message(
        &mut ctl_tx,
        &Envelope::new(Message::Offer {
            manifest: manifest.clone(),
        }),
    )
    .await
    .unwrap();
    match read_message(&mut ctl_rx).await.unwrap().message {
        Message::Accept { .. } => {}
        other => panic!("expected Accept, got {other:?}"),
    }
    let (mut data_tx, _data_rx) = conn.open_bi().await.unwrap();
    let mut f = fs::File::open(source).unwrap();
    use std::io::{Read, Seek, SeekFrom};
    for (ci, chunk) in manifest.files[0]
        .chunks
        .iter()
        .take(send_chunks)
        .enumerate()
    {
        let mut buf = vec![0u8; chunk.length as usize];
        f.seek(SeekFrom::Start(chunk.offset)).unwrap();
        f.read_exact(&mut buf).unwrap();
        write_data_frame(&mut data_tx, 0, ci as u32, &buf)
            .await
            .unwrap();
    }
    write_data_frame(&mut data_tx, u32::MAX, 0, b"sentinel")
        .await
        .unwrap();
}

/// Scenario: interrupted transfer — resume must beat restarting from zero.
/// Drives a real kill-and-resume through the engine API over loopback: a
/// session carrying ~40% of the file breaks, then a full `send_with`
/// reconciles against the staged chunks and completes. Reports the resumed
/// session's time; the notes carry the measured full-restart cost that
/// resume avoided.
fn scenario_resume(size_mb: u64) -> Result<BenchResult, String> {
    let src = temp_root("rs-src");
    let data = pseudo_bytes(0x42, size_mb as usize * 1024 * 1024);
    let file = write_fixture(&src, "resume.dat", &data);

    let runtime = tokio::runtime::Runtime::new().map_err(|e| e.to_string())?;
    let (full, resumed, staged_fraction) = runtime.block_on(async {
        let identity =
            DeviceIdentity::load_or_create(&temp_root("rs-id")).map_err(|e| e.to_string())?;
        let server_id = transport::server_identity(&identity).map_err(|e| e.to_string())?;
        let fingerprint = identity.fingerprint();
        let cert = CertificateDer::from(server_id.cert_der().to_vec());

        // Reference: full transfer time (what a restart would pay again).
        let started = Instant::now();
        loopback_transfer(&identity, &server_id, std::slice::from_ref(&file), 4).await?;
        let full = started.elapsed();

        // Kill-and-resume: two sessions on the same target/staging.
        let target = temp_root("rs-target");
        let staging = temp_root("rs-staging");
        let opts = ReceiveOptions::new(target.clone(), staging);
        let (addr, mut receiver) = serve_sessions(server_id, opts, 2).await;

        // Session 1: ~40% of the chunks, then the session breaks.
        let manifest = build_manifest(std::slice::from_ref(&file), &ChunkParams::default())
            .map_err(|e| e.to_string())?;
        let total_chunks = manifest.files[0].chunks.len();
        let partial = total_chunks * 2 / 5;
        {
            let (_ep, conn) = transport::dial(addr, &fingerprint, cert.clone())
                .await
                .map_err(|e| e.to_string())?;
            rogue_partial(&conn, &fingerprint, &manifest, &file, partial).await;
            // The receiver fails on the sentinel (expected for a broken
            // session); only then may the connection go away.
            let first = receiver.recv().await.unwrap();
            debug_assert!(first.is_err(), "broken session must fail: {first:?}");
        }

        // Session 2: a real send reconciles against the staged chunks and
        // completes the transfer — time only the resumed session.
        let started = Instant::now();
        {
            let (_ep, conn) = transport::dial(addr, &fingerprint, cert)
                .await
                .map_err(|e| e.to_string())?;
            transfer::send_with(
                &conn,
                std::slice::from_ref(&file),
                &SenderInfo {
                    name: "bench".into(),
                    fingerprint: fingerprint.clone(),
                },
                &transfer::SendOptions {
                    parallelism: 4,
                    progress: None,
                },
            )
            .await
            .map_err(|e| e.to_string())?;
        }
        let resumed = started.elapsed();
        let second = receiver
            .recv()
            .await
            .ok_or("receiver task ended early")?
            .map_err(|e| e.to_string())?;
        if second != data.len() as u64 {
            return Err(format!(
                "resumed transfer delivered {second} bytes, expected {}",
                data.len()
            ));
        }
        // Integrity is part of the benchmark: the resumed file must be
        // byte-identical (whole-file hash from the manifest, which was
        // itself derived from the source content).
        let got = target.join("resume.dat");
        let got_hash = hash_reader(fs::File::open(&got).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        if got_hash != manifest.files[0].hash {
            return Err("resumed transfer produced a corrupt file".to_string());
        }
        Ok::<_, String>((full, resumed, partial as f64 / total_chunks.max(1) as f64))
    })?;
    Ok(BenchResult {
        scenario: "resume".into(),
        bytes: data.len() as u64,
        elapsed_ms: resumed.as_millis(),
        throughput_mbps: mb(data.len() as u64) / resumed.as_secs_f64().max(0.001),
        notes: format!(
            "loopback, session killed after ~{:.0}% of chunks; resume = {:?} vs full restart = {:?}",
            staged_fraction * 100.0,
            resumed,
            full
        ),
    })
}

/// Scenario: plain `fs::copy` of the same fixtures — the "just copying on
/// this machine" baseline that puts engine numbers in context.
fn scenario_copy_baseline(size_mb: u64) -> Result<BenchResult, String> {
    let src = temp_root("cb-src");
    let data = pseudo_bytes(0xB16, size_mb as usize * 1024 * 1024);
    let file = write_fixture(&src, "big.dat", &data);
    let dst = temp_root("cb-dst");
    let started = Instant::now();
    let bytes = fs::copy(&file, dst.join("big.dat")).map_err(|e| e.to_string())?;
    let mut notes = "plain fs::copy, same disk".to_string();
    // Small-file floor: 2000 × 2 KiB plain copies — the per-file cost ANY
    // tool pays on this OS/filesystem.
    let small_src = temp_root("cb-small-src");
    let small_dst = temp_root("cb-small-dst");
    let mut small_bytes = 0u64;
    let small_started = Instant::now();
    for i in 0..2000u32 {
        let p = write_fixture(
            &small_src,
            &format!("f{i}.bin"),
            &pseudo_bytes(i as u64 + 1, 2048),
        );
        small_bytes +=
            fs::copy(&p, small_dst.join(format!("f{i}.bin"))).map_err(|e| e.to_string())?;
    }
    let small_ms = small_started.elapsed().as_millis();
    notes.push_str(&format!(
        "; small-file floor: 2000 x 2KiB plain copy = {small_ms} ms"
    ));
    let _ = small_bytes;
    Ok(to_result("copy-baseline", bytes, started, &notes))
}

/// Run the requested scenarios (None = all) and return the report.
pub fn run(scenario: Option<&str>, size_mb: u64) -> Result<BenchReport, String> {
    let mut report = env_report();
    let mut push = |r: Result<BenchResult, String>| match r {
        Ok(result) => {
            println!(
                "{:<12} {:>8.1} MB  {:>8} ms  {:>8.1} MB/s  {}",
                result.scenario,
                mb(result.bytes),
                result.elapsed_ms,
                result.throughput_mbps,
                result.notes
            );
            report.results.push(result);
        }
        Err(e) => eprintln!("bench: scenario failed: {e}"),
    };
    let run_all = scenario.is_none();
    if run_all || scenario == Some("copy-baseline") {
        push(scenario_copy_baseline(size_mb));
    }
    if run_all || scenario == Some("big-file") {
        push(scenario_big_file(size_mb));
    }
    if run_all || scenario == Some("many-small") {
        push(scenario_many_small());
    }
    if run_all || scenario == Some("resume") {
        push(scenario_resume(size_mb));
    }
    if report.results.is_empty() {
        return Err(format!(
            "unknown scenario '{}' (copy-baseline | big-file | many-small | resume)",
            scenario.unwrap_or("")
        ));
    }
    Ok(report)
}

/// Persist the report as pretty JSON.
pub fn write_output(report: &BenchReport, output: Option<&Path>) -> Result<(), String> {
    let json = serde_json::to_string_pretty(report).map_err(|e| e.to_string())?;
    match output {
        Some(path) => fs::write(path, json).map_err(|e| e.to_string())?,
        None => println!("{json}"),
    }
    Ok(())
}
