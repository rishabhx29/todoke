# Two-device LAN test — runbook

The first real-network test (everything so far: loopback + Docker bridge).
Goal: proof the discovery + transfer stack works on real Wi-Fi, plus the
first publishable benchmark claim. ~30 minutes with two devices on the same
network.

## What you need

- Device A: this Windows machine (built binaries in `dasho-app/target/release/` or `target/debug/` — build with `cargo build --release -p todoke-cli`)
- Device B: any laptop (Windows/macOS/Linux) or the same machine's second network identity won't do — it must be a second device
- Both on the same Wi-Fi (or B via phone hotspot with A also on it — mDNS needs the same L2 segment; guest networks often block multicast, prefer a private SSID)

## Steps

### 1. One-time firewall allowance (Windows, admin PowerShell)

```powershell
netsh advfirewall firewall add rule name="Todoke mDNS" dir=in action=allow protocol=UDP localport=5353
netsh advfirewall firewall add rule name="Todoke QUIC" dir=in action=allow protocol=UDP
```

(The QUIC rule is port-agnostic because serve binds an ephemeral port;
narrow `localport=<port>` after you see the port it prints, if you want.)

### 2. Receiver on device A

```
todoke serve --target C:\incoming
```

Note the line it prints: name + fingerprint + address. Leave it running.

### 3. Sender on device B

```
todoke devices          # should list device A by name + fingerprint
todoke send --to <name-or-fp> C:\path\to\bigfile.bin C:\path\to\photos\
```

Watch the progress bar run at LAN speed.

### 4. The claims to record (paste into bench results)

| scenario | command on A (receiving) | what to note |
|---|---|---|
| big file | 2 GB movie file | wall-clock, throughput MB/s, NIC link rate |
| many small | folder of ~2,000 photos | wall-clock vs `robocopy` on the same folder |
| resume | start a 5 GB send, kill the sender mid-flight (Ctrl-C), rerun same command | "100% skipped, X s" line |
| receipt | any completed transfer | per-file BLAKE3 lines match on both ends |

For apples-to-apples numbers run the same two transfers with:
- **LocalSend** (if installed) — the direct competitor
- **scp** (B is Linux/macOS: `scp -r folder A-user@A-ip:~/incoming/`)
- plain **robocopy/SMB copy** if B mounts a share

`todoke bench` results JSON from device A (loopback) is the engine ceiling;
the LAN numbers will be lower and bounded by the Wi-Fi link — that's the
honest comparison, and the reason the bench harness prints link context.

## If discovery fails

1. `todoke devices` empty on B → most likely multicast blocked (guest Wi-Fi,
   AP isolation, VPN adapter). Fix: use `--advertise-ip <A's IP>` on serve +
   direct mode on B:
   `todoke send --addr <A-ip>:<port> --fingerprint <fp> --cert-b64 <b64> ...`
   (all three values are in serve's `--json` line on A)
2. Windows Firewall prompt on first run — allow on private networks.
3. VPN clients (Tailscale/ZeroTier/Windscribe) hijack routes — disconnect.

## After the test

Paste the numbers + the two device names into `.scratch/v1-foundation/` —
they become the README benchmark table and the website's "Measured" section
context (loopback 108 MB/s = engine ceiling; LAN number = your real claim).
