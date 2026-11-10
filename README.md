# Todoke 届け

**Files that arrive.** A free, open-source, cross-platform desktop app for sharing anything — files, folders, videos, whole drives — between devices at local-network speed, with no cloud, no accounts, and no size limits. Every transfer ends with a BLAKE3 receipt: proof of what arrived, not a promise.

Todoke (届け, "it arrives") is the rename of the project formerly prototyped as Dasho.

## Status (2026-11)

| Track | State |
|---|---|
| Transfer engine (`todoke-core`) | ✅ v1 complete — 72 tests green on Windows + Debian/musl Linux, cross-OS container-verified |
| CLI (`todoke-cli`) | ✅ serve / send / devices / bench |
| Desktop GUI | ✅ Tauri 2 + Svelte 5, e2e-verified, Apple-theme restyle in flight |
| Website (`todoke.tech`) | ✅ zen edition live in-repo, Lighthouse 90/100/100/100 |
| Installers + auto-update | 🔜 CI workflow ready; signing = owner decision |
| Cross-network relay (Iroh mesh), web receiver, mobile | 🔜 v1.x roadmap |

Work lands on feature branches (`app`, `gui`, `website` worktrees) and merges
to `main` per release.

## Measured, not promised

Every claim is reproducible with the shipped harness (`todoke bench`,
scenarios + fixtures + environment metadata in JSON):

- **108 MB/s** big-file throughput (256 MB, 4 parallel streams, loopback —
  gigabit wire ceiling is 112)
- **2,000 files in 2.8 s** (2 KiB each — the photo-library case, at the OS
  copy floor)
- **100% of staged bytes skipped** on resume — kill a transfer mid-flight,
  rerun, it continues instead of restarting

## Design decisions that make it fast *and* trustworthy

1. **Parallel stream striping** sized to the receiver's Accept — one stream
   is how other tools crawl through Wi-Fi
2. **fastcdc content-defined chunking + BLAKE3** — verify per chunk, resume
   per chunk, receipt per file
3. **Ed25519 device identities + TOFU pinning** — deterministic certs, no
   accounts, nothing to breach; the fingerprint is readable over a phone call
4. **Reconcile-from-disk resume** — no state file to corrupt; the disk is the
   state
5. **QUIC (quinn) + mDNS discovery** — no central server, no relay in v1

## Domains & handles

| Asset | Status |
|---|---|
| `todoke.tech` | primary — to register (~$15/yr) |
| `todoke.web.id` | defensive — verified available |
| GitHub `todokeapp` / npm `todoke-cli` | verified available — to claim at publish |

## Stack (rationale in [docs/decisions.md](docs/decisions.md))

- **Engine:** Rust — quinn (QUIC, multi-stream) · mdns-sd (LAN discovery) · fastcdc + BLAKE3 · ed25519 (TOFU)
- **App:** Tauri 2 + Svelte 5 + TypeScript
- **Distribution:** GitHub Actions (CI matrix + tauri-action release bundles: NSIS / dmg / AppImage+deb); auto-updates via GitHub Releases; Azure Trusted Signing ($9.99/mo) for Windows and Apple Developer ($99/yr) for macOS are owner decisions pending

## Repo conventions

See `AGENTS.md` and `docs/agents/` — issues live as local markdown under
`.scratch/<feature>/`, five canonical triage labels, single-context domain
docs.
