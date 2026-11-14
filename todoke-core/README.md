# todoke-core — the Todoke transfer engine

The headless engine behind Todoke ("it arrives"): LAN device discovery, encrypted
identity-verified transport, content-defined chunking, parallel multi-file
transfers, resume across reconnects and crashes, and per-transfer verification
receipts. The CLI (`todoke-cli`) and the future GUI both consume this crate's
public async API.

Dual-licensed MIT OR Apache-2.0.

## Architecture (one tour)

- **identity** — every device has an Ed25519 keypair (generated on first run,
  persisted in the app-data dir) and a human-checkable fingerprint. A
  trust-on-first-use store remembers peers and rejects a known name presenting
  a different key.
- **transport** — QUIC (quinn). Each device's TLS certificate is a pure
  function of its identity key (fixed validity, serial from the key, SAN
  `todoke-<fp>.todoke.internal`, self-signed with the device key). Peers pin
  the advertised certificate as their only root: no CA, no dangerous rustls
  modes, and an impostor without the peer's key cannot present a certificate
  that matches the pinned fingerprint. The transport sits behind a small
  trait so the future Iroh mesh slots in beside LAN QUIC.
- **discovery** — mDNS (`_todoke._udp.local.`) with TXT records carrying the
  device name, fingerprint, and the certificate (chunked). Browsing yields
  typed, validated device records. Windows note: the firewall silently drops
  mDNS from unsigned binaries on real interfaces (loopback works) — see
  examples/mdns_probe.rs and the firewall note below.
- **chunk** — fastcdc content-defined chunking + BLAKE3 hashes per chunk and
  per file. Identical content yields identical manifests regardless of where
  the tree lives.
- **manifest / protocol** — the sender offers a content-addressed manifest
  (file list + chunk hash lists). Control messages are length-prefixed
  versioned JSON; chunk data is compact binary frames. Unknown versions and
  unknown message types fail loudly.
- **transfer** — the sender stripes (file, chunk) work across N concurrent
  QUIC streams (default 4, bounded by the receiver). The receiver verifies
  every chunk (BLAKE3) before it touches disk, stages under
  `<staging>/<manifest-hash>/`, and finalizes each file into the target only
  when the whole file verifies — collision-safe, never overwriting.
- **resume** — on (re)connect the receiver rebuilds its have-list from disk
  (each staged chunk must still hash correctly), so crashes on either side
  cost nothing: the restarted sender sends only the remainder. Damaged
  chunks are answered with a Missing list and re-requested (bounded rounds).
- **receipts** — at completion the receiver returns a per-file receipt
  (path, size, whole-file BLAKE3, chunk count); the sender verifies it
  against its own manifest before reporting success. Nothing is "done"
  until it is verified.

## Use it

```bash
cargo build --release
# Machine A — receive:
./target/release/todoke serve --target ./incoming
# Machine B — send (discover by device name):
./target/release/todoke send ./big-video.mp4 --to laptop
# Or address directly (no discovery; useful for scripting):
./target/release/todoke send ./big-video.mp4 \
  --addr 192.168.1.20:52341 --fingerprint ABCD1234-... --cert-b64 ...
todoke devices          # what's on this network?
todoke bench            # engine benchmarks (loopback)
```

Identity lives in the OS app-data dir (`%APPDATA%/todoke` on Windows);
override with `TODOKE_DATA_DIR` for tests or portable use.

## Windows firewall (first run)

mDNS announcement and inbound QUIC are blocked for unsigned binaries until
you allow them. One-time, admin PowerShell:

```powershell
netsh advfirewall firewall add rule name="Todoke" dir=in action=allow protocol=UDP localport=5353
netsh advfirewall firewall add rule name="Todoke QUIC" dir=in action=allow protocol=TCP localport=49152-65535
```

(The future installer pre-adds these.) SmartScreen reputation is a separate,
time-based thing — signed releases fix it.

## Benchmarks

```bash
todoke bench                      # all scenarios, 64 MB default
TODOKE_BENCH_SIZE_MB=256 todoke bench --scenario big-file --output results.json
```

Scenarios: `copy-baseline` (plain fs::copy of the same fixtures — the
context line), `big-file` (throughput ceiling), `many-small` (2000 × 2 KiB —
per-file overhead), `resume` (restart cost that resume avoids). Loopback
numbers measure engine overhead only; publish LAN numbers from two real
machines using the same JSON schema. Re-runs should agree within ±10%.

Release-build results (Windows x64, NVMe, loopback, 4 streams):
big-file **≈ 105–115 MB/s** (at gigabit wire speed — the engine is not the
bottleneck for real LAN use); many-small **≈ 2.8 s** for 2000 files vs a
**1.8 s plain-copy floor** for the same fixture — within 1.6× of the OS
per-file cost, which every tool on the platform pays. Remaining levers,
in order: sender-side chunk read batching for multi-chunk files, BLAKE3
parallel verify, container bundling for many-small semantics.

## Development

```bash
cargo test --workspace      # 15 suites: unit + integration + e2e
cargo clippy --workspace --all-targets -- -D warnings
```

CI runs fmt/clippy/tests on Windows, macOS and Linux. Examples:
`mdns_probe` (diagnose discovery/firewall), `endpoint_drop_probe`
(quinn endpoint lifetime semantics).
