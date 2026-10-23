# Dasho

**Files that move.** A free, open-source, cross-platform desktop app for sharing anything — files, folders, videos, whole drives — between devices at local-network speed, with no cloud, no compression, and no size limits.

## Why

Existing tools fail at one of: speed (single-stream transfers waste the link), reliability (no resume across sleep/network changes), trust (closed-source after the SHAREit era), or reach (no way to receive without installing). Dasho targets all four.

## Positioning

- The fastest transfer engine available — multi-stream, small-file packing, delta/dedupe — with **published benchmarks** to prove it
- Transfers that never fail: resume across reboots, network switches, and sleep; per-file checksum verification receipts
- Your devices as one mesh: pair once, send across networks, no cables, no cloud
- Receive without installing: QR-code web receiver + portable single-binary mode
- Entirely free, forever — serverless-first architecture makes that sustainable

## Domains & handles

| Asset | Status |
|---|---|
| `dasho.tech` | primary domain — to register |
| `dasho.web.id` / `dasho.my.id` | defensive — to register |
| GitHub `dashoapp` | available — to claim |
| npm org `dasho` / package `dasho-cli` | `dasho` is a dormant personal lib; use org or `-cli` |
| X `@dasho` | check manually before launch |

## Stack (see [docs/decisions.md](docs/decisions.md) for rationale)

- **App:** Tauri 2 + TypeScript (Svelte 5 or SolidJS)
- **Engine:** Rust — Iroh (mesh + NAT traversal) · quinn (QUIC, multi-stream) · mdns-sd (LAN discovery) · fastcdc + BLAKE3 (chunking/dedupe)
- **Distribution:** GitHub Actions + tauri-action, auto-updates via GitHub Releases; Azure Artifact Signing ($9.99/mo) for Windows, Apple Developer ($99/yr) for macOS

## Repo conventions

See `AGENTS.md` and `docs/agents/` — issues live as local markdown under `.scratch/<feature>/`, five canonical triage labels, single-context domain docs.
