//! Ticket 08 acceptance tests: the CLI as users run it — real processes,
//! real transfers on the same machine. Discovery itself is covered by the
//! todoke-core discovery suite; the e2e here uses direct addressing so it is
//! deterministic regardless of multicast/firewall quirks.

use std::{fs, path::PathBuf, process::Stdio, time::Duration};

use assert_cmd::Command;

fn tmpdir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("todoke-cli-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn help_documents_every_command() {
    let out = Command::cargo_bin("todoke")
        .unwrap()
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    for cmd in ["serve", "send", "devices", "bench"] {
        assert!(text.contains(cmd), "help must document `{cmd}`: {text}");
    }
    assert!(
        text.contains("receive"),
        "help should describe receive mode"
    );
}

#[test]
fn devices_json_outputs_an_array() {
    let data = tmpdir("devices-data");
    let out = Command::cargo_bin("todoke")
        .unwrap()
        .args(["devices", "--json", "--timeout", "1"])
        .env("TODOKE_DATA_DIR", &data)
        .output()
        .unwrap();
    assert!(out.status.success());
    let parsed: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("devices --json must print a JSON array");
    assert!(parsed.is_array());
}

#[test]
fn serve_send_end_to_end_with_receipt() {
    let receiver_data = tmpdir("e2e-recv-data");
    let sender_data = tmpdir("e2e-send-data");
    let target = tmpdir("e2e-target");
    let staging = tmpdir("e2e-staging");
    let src = tmpdir("e2e-src");
    fs::write(src.join("lecture.mp4"), vec![7u8; 300 * 1024]).unwrap();
    fs::write(src.join("notes.txt"), b"week 1 notes").unwrap();

    // Receiver process, JSON mode: its ready-line carries everything the
    // sender needs for direct addressing (addr, fingerprint, certificate).
    let exe = Command::cargo_bin("todoke")
        .unwrap()
        .get_program()
        .to_os_string();
    let mut serve = std::process::Command::new(exe)
        .args([
            "serve",
            "--target",
            target.to_str().unwrap(),
            "--staging",
            staging.to_str().unwrap(),
            "--name",
            "cli-e2e-dev",
            "--json",
        ])
        .env("TODOKE_DATA_DIR", &receiver_data)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("serve spawns");

    let serve_stdout = serve.stdout.take().unwrap();
    let ready = read_line_with_timeout(serve_stdout, Duration::from_secs(20))
        .expect("serve must print a ready-line within 20s");
    let ready_json: serde_json::Value = serde_json::from_str(ready.trim())
        .unwrap_or_else(|e| panic!("ready-line must be JSON ({e}): {ready}"));
    assert_eq!(ready_json["serving"], serde_json::json!(true));
    let addr = ready_json["addr"].as_str().unwrap().to_string();
    let fingerprint = ready_json["fingerprint"].as_str().unwrap().to_string();
    let cert_b64 = ready_json["cert_b64"].as_str().unwrap().to_string();

    // Sender process: direct-addressed transfer with a receipt.
    let out = Command::cargo_bin("todoke")
        .unwrap()
        .args([
            "send",
            src.join("lecture.mp4").to_str().unwrap(),
            src.join("notes.txt").to_str().unwrap(),
            "--addr",
            &addr,
            "--fingerprint",
            &fingerprint,
            "--cert-b64",
            &cert_b64,
            "--json",
        ])
        .env("TODOKE_DATA_DIR", &sender_data)
        .output()
        .expect("send runs");
    let send_text = String::from_utf8_lossy(&out.stdout).to_string();
    let send_json: serde_json::Value = serde_json::from_str(send_text.trim())
        .unwrap_or_else(|e| panic!("send --json must print JSON ({e}): {send_text}"));
    assert_eq!(
        send_json["ok"],
        serde_json::json!(true),
        "send output: {send_json}"
    );
    assert_eq!(send_json["files"].as_array().unwrap().len(), 2);

    // Files landed byte-intact (hashes verified against the receipt).
    for f in send_json["files"].as_array().unwrap() {
        let rel = f["path"].as_str().unwrap();
        let dest = target.join(rel);
        assert!(dest.is_file(), "missing {rel} in target");
        if rel.ends_with(".txt") {
            assert_eq!(fs::read(&dest).unwrap(), b"week 1 notes");
        } else {
            assert_eq!(fs::metadata(&dest).unwrap().len(), 300 * 1024);
        }
    }
    let _ = serve.kill();
    let _ = serve.wait();
}

#[test]
fn send_to_unknown_device_fails_with_guidance() {
    let data = tmpdir("unknown-data");
    let src = tmpdir("unknown-src");
    fs::write(src.join("x.txt"), b"hi").unwrap();
    let out = Command::cargo_bin("todoke")
        .unwrap()
        .args([
            "send",
            src.join("x.txt").to_str().unwrap(),
            "--to",
            "definitely-not-a-device",
            "--discovery-secs",
            "2",
        ])
        .env("TODOKE_DATA_DIR", &data)
        .output()
        .unwrap();
    assert!(!out.status.success(), "unknown device must fail");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("no device matching"),
        "stderr should guide: {err}"
    );
    assert!(
        err.contains("todoke devices"),
        "stderr should point at `todoke devices`"
    );
}

#[test]
fn send_missing_path_fails_with_guidance() {
    let data = tmpdir("missingpath-data");
    let out = Command::cargo_bin("todoke")
        .unwrap()
        .args(["send", "does-not-exist-anywhere.txt"])
        .env("TODOKE_DATA_DIR", &data)
        .output()
        .unwrap();
    assert!(!out.status.success(), "missing path must fail");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("path not found"),
        "stderr should explain: {err}"
    );
}

#[test]
fn send_only_empty_directories_fails_with_nothing_to_send() {
    let data = tmpdir("emptydir-data");
    let src = tmpdir("emptydir-src");
    fs::create_dir_all(src.join("only-empty/inner")).unwrap();
    let out = Command::cargo_bin("todoke")
        .unwrap()
        .args(["send", src.join("only-empty").to_str().unwrap()])
        .env("TODOKE_DATA_DIR", &data)
        .output()
        .unwrap();
    assert!(!out.status.success(), "a file-less input set must fail");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("nothing to send"),
        "stderr should explain: {err}"
    );
}

/// Read one line from the child's stdout with a hard deadline: a blocked
/// read() can't be interrupted by a deadline check between reads, so the
/// reader runs on a thread and we bail via channel timeout.
fn read_line_with_timeout<R: std::io::Read + Send + 'static>(
    mut reader: R,
    timeout: Duration,
) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match reader.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    line.push(byte[0]);
                    if byte[0] == b'\n' {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(String::from_utf8_lossy(&line).to_string());
    });
    rx.recv_timeout(timeout).ok()
}
