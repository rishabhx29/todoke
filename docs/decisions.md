# Decision Record

Log of decisions made during research and planning (2026-10). Sources and full competitive analysis live in the planning session notes; this is the working summary.

## Product

- **Name:** Dasho. Chosen for: sounds fast, one clean word, no trademark shadows, all four target domains free at standard price, playful-but-professional tone for a student/20s audience on X/Reddit/Discord.
- **Shape:** "The engine" — fastest transfer + unkillable reliability + your-devices mesh — with the no-install web receiver embedded. Desktop-first (Windows/macOS/Linux); mobile later.
- **Free, entirely, forever:** serverless-first architecture (mDNS discovery, P2P hole-punching, no accounts, no file-touching servers) makes free sustainable. Running cost target: ~$25/mo + $99/yr.

## Competitive landscape (why this isn't "another LocalSend")

- LocalSend (~95k stars, 35M+ downloads) owns "simple local sharing" — do not compete on simplicity. Compete on dimensions it doesn't do: benchmarked speed (multi-stream, small-file packing, dedupe), resume-everything, integrity receipts, cross-network mesh, no-install receiving.
- Documented incumbent weaknesses: LocalSend single-stream speed failures (Windows→Android 1–2 Mbps while FTP saturates), no resume across network changes, no remote transfer with GUI.
- PairDrop/Blip/Send Anywhere/Feem/croc each cover a slice; none combine speed + reliability + mesh + receive-without-install.
- PC migration (Migration Assistant for Windows) is an unserved niche — candidate later wedge, not v1.

## Stack

- **Tauri 2 + Rust** over Flutter/Electron: ~2–5 MB installers (vs 50+ MB), <0.5s startup, lowest RAM, one language for UI backend and engine. Frontend: Svelte 5 or SolidJS with virtualized lists.
- **Iroh** (n0 computer) for the mesh: dial-by-public-key, QUIC direct connections, NAT hole-punching, encrypted relay fallback, `iroh-blobs` for BLAKE3-verified streaming. Wrap behind our own trait (pre-1.0 API churn isolation).
- **quinn** for LAN QUIC: parallel streams (multi-stream striping), TLS 1.3 built in, connection migration. Keep on current patches (2026 memory-exhaustion DoS was patched).
- **mdns-sd** for LAN discovery; **fastcdc + BLAKE3** for content-defined chunking/dedupe (restic's pattern); **zstd** only for compressible types — never recompress media.
- Multipath bonding (Ethernet+Wi-Fi) and USB/Thunderbolt cable mode are later milestones, not v1.
- **Open-source the engine** (MIT/Apache-2.0) from day one — trust is the entry fee post-SHAREit.

## Distribution

- GitHub Actions + tauri-action build matrix; auto-updates via updater plugin + `latest.json` on GitHub Releases (zero servers).
- Windows signing: Azure Artifact Signing $9.99/mo (individual devs OK, builds SmartScreen reputation).
- macOS: Apple Developer Program $99/yr, Developer ID + notarization.
- Linux: AppImage + .deb.

## Known gotchas

- First-run firewall/permission prompts: pre-add Windows firewall rule in installer; expect macOS local-network prompt; SmartScreen reputation takes time.
- Android SAF slowness (the LocalSend complaint) — irrelevant for desktop v1, plan for it at mobile time.
- Don't do: Bluetooth as a headline (1–3 MB/s), chat/social features, accounts, telemetry-by-default.

## Build order

1. Headless Rust engine crate + CLI harness (no UI)
2. Benchmark harness from day one — the brand is "fastest", claims must be reproducible (vs LocalSend, scp; big-file / many-small-file / resume scenarios)
3. Tauri shell: device list, send/receive, speed graph, two-pane picker
4. Persistent queue + resume + verification receipts
5. Pairing/QR + mesh (Iroh)
6. No-install web receiver + portable exe
7. Later: cable mode, multipath bonding, mobile

## Rename: Dasho -> Todoke (2026-10-27)

- **New name: Todoke** (届け/届く, "it arrives / deliver!") — chosen by the owner's brief (Japanese, product-resembling, cool-sounding, .tech free). Meaning aligns with the trust story: files that ARRIVE, verified by receipts. Runner-up considered: Shiden (紫電, purple lightning).
- **Verified free at decision time**: todoke.tech, todoke.web.id (RDAP via radix.host — note rdap.nic.tech serves HTML, calibrate against the IANA bootstrap); GitHub `todokeapp`, npm `todoke-cli`.
- Renamed: crates (todoke-core/todoke-cli), binary (`todoke`), mDNS service type (`_todoke._udp.local.`), TLS SAN suffix (`todoke.internal`), app-data dir, website copy/domain. Physical worktree paths (C:\Rishabh\dasho-app / dasho-website) and branch names (app/website) are unchanged for tooling stability. Historical docs saying "Dasho" mean Todoke.
