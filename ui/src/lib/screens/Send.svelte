<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import {
    cancelTransfer,
    listDevices,
    sendFiles,
    statPaths,
    type Device,
    type Receipt,
    type SendEvent,
  } from "../api";
  import { app, go, withManualPeers } from "../state.svelte";
  import { icons } from "../icons";
  import { copyText } from "../clipboard";
  import { etaString, formatBytes, formatSpeed, shortFp } from "../format";

  type Phase = "pick" | "sending" | "receipt" | "failed";

  let phase = $state<Phase>("pick");
  let items = $state<string[]>([]);
  let stats = $state<{ files: number; totalBytes: number; skipped: number } | null>(null);
  let statting = $state(false);

  let devices = $state<Device[]>([]);
  let selectedFp = $state<string | null>(null);
  let deviceTimer: ReturnType<typeof setTimeout> | undefined;

  let session = $state<number | null>(null);

  // Raw progress accumulators (non-reactive, flushed to $state on a timer
  // so chunk-rate events never thrash the renderer).
  let raw = {
    sentBytes: 0,
    lastBytes: 0,
    lastAt: 0,
    perFile: new Map<string, { size: number; sent: number }>(),
  };
  let sentBytes = $state(0);
  let perFile = $state<Record<string, { size: number; sent: number }>>({});
  let speed = $state(0); // bytes/sec, smoothed
  let flushTimer: ReturnType<typeof setInterval> | undefined;

  let receipt = $state<Receipt | null>(null);
  let failMessage = $state<string | null>(null);
  let cancelledByUser = $state(false);

  const selectedDevice = $derived(devices.find((d) => d.fingerprint === selectedFp) ?? null);
  const totalBytes = $derived(stats?.totalBytes ?? 0);
  const fraction = $derived(
    totalBytes > 0 ? Math.min(1, sentBytes / totalBytes) : 0,
  );
  const remaining = $derived(Math.max(0, totalBytes - sentBytes));
  const activeFiles = $derived(
    Object.entries(perFile).filter(([, f]) => f.sent < f.size).slice(0, 4),
  );
  const doneCount = $derived(
    receipt
      ? receipt.files.length
      : Object.values(perFile).filter((f) => f.sent >= f.size).length,
  );

  // ---------------------------------------------------------------- inputs

  async function addFiles() {
    const picked = await open({ multiple: true, title: "Choose files to send" });
    if (!picked) return;
    addPaths(Array.isArray(picked) ? picked : [picked]);
  }

  async function addFolder() {
    const picked = await open({
      directory: true,
      multiple: true,
      title: "Choose folders to send",
    });
    if (!picked) return;
    addPaths(Array.isArray(picked) ? picked : [picked]);
  }

  function addPaths(paths: string[]) {
    const known = new Set(items);
    items = [...items, ...paths.filter((p) => !known.has(p))];
    void refreshStats();
  }

  function removePath(p: string) {
    items = items.filter((x) => x !== p);
    void refreshStats();
  }

  function clearItems() {
    items = [];
    stats = null;
  }

  async function refreshStats() {
    if (items.length === 0) {
      stats = null;
      return;
    }
    statting = true;
    try {
      stats = await statPaths(items);
    } catch {
      stats = null; // totals are best-effort; sending still works
    } finally {
      statting = false;
    }
  }

  // ------------------------------------------------------------- devices

  $effect(() => {
    if (phase !== "pick") return;
    let alive = true;
    const poll = async () => {
      try {
        const found = withManualPeers(await listDevices(3000));
        if (!alive) return;
        devices = found;
        if (!selectedFp || !found.some((d) => d.fingerprint === selectedFp)) {
          const wanted = app.pendingDestination;
          selectedFp =
            found.find((d) => d.fingerprint === wanted)?.fingerprint ??
            found[0]?.fingerprint ??
            null;
          app.pendingDestination = null;
        }
      } catch {
        /* transient discovery failures are fine; next tick retries */
      } finally {
        if (alive) deviceTimer = setTimeout(poll, 4000);
      }
    };
    void poll();
    return () => {
      alive = false;
      if (deviceTimer) clearTimeout(deviceTimer);
    };
  });

  // ------------------------------------------------------------ drag-drop

  let dragOver = $state(false);

  $effect(() => {
    const unlisten = getCurrentWebview().onDragDropEvent((e) => {
      const kind = e.payload.type;
      if (kind === "enter" || kind === "over") {
        if (phase === "pick") dragOver = true;
      } else if (kind === "drop") {
        dragOver = false;
        if (phase === "pick" && e.payload.paths.length > 0) {
          addPaths(e.payload.paths);
        }
      } else {
        dragOver = false;
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  });

  // ---------------------------------------------------------------- send

  function handleEvent(m: SendEvent) {
    if (m.event === "fileStarted") {
      raw.perFile.set(m.data.path, { size: m.data.size, sent: 0 });
    } else if (m.event === "chunkSent") {
      raw.sentBytes += m.data.bytes;
      const f = raw.perFile.get(m.data.path);
      if (f) f.sent += m.data.bytes;
    } else if (m.event === "fileSent") {
      const f = raw.perFile.get(m.data.path);
      if (f) f.sent = f.size;
    } else if (m.event === "finished") {
      flush();
      receipt = m.data.receipt;
      stopFlushing();
      phase = "receipt";
    } else if (m.event === "failed") {
      flush();
      stopFlushing();
      failMessage = m.data.message;
      phase = "failed";
    }
  }

  function flush() {
    sentBytes = raw.sentBytes;
    perFile = Object.fromEntries(raw.perFile);
  }

  function stopFlushing() {
    if (flushTimer) {
      clearInterval(flushTimer);
      flushTimer = undefined;
    }
  }

  async function startSend() {
    const device = selectedDevice;
    if (!device || items.length === 0 || phase === "sending") return;

    raw = { sentBytes: 0, lastBytes: 0, lastAt: Date.now(), perFile: new Map() };
    sentBytes = 0;
    perFile = {};
    speed = 0;
    receipt = null;
    failMessage = null;
    cancelledByUser = false;

    try {
      session = await sendFiles(items, device, handleEvent);
    } catch (e) {
      failMessage = e instanceof Error ? e.message : String(e);
      phase = "failed";
      return;
    }
    phase = "sending";
    flushTimer = setInterval(() => {
      const now = Date.now();
      const instant = ((raw.sentBytes - raw.lastBytes) * 1000) / Math.max(1, now - raw.lastAt);
      raw.lastBytes = raw.sentBytes;
      raw.lastAt = now;
      if (raw.sentBytes > 0) speed = speed === 0 ? instant : speed * 0.6 + instant * 0.4;
      flush();
    }, 200);
  }

  async function cancelSend() {
    if (session == null) return;
    cancelledByUser = true;
    try {
      await cancelTransfer(session);
    } catch {
      /* session may have just completed on its own */
    }
  }

  function reset() {
    stopFlushing();
    phase = "pick";
    session = null;
    receipt = null;
    failMessage = null;
    sentBytes = 0;
    perFile = {};
    speed = 0;
  }

  const fileName = (p: string) => p.replaceAll("\\", "/").split("/").pop() ?? p;
  const dirName = (p: string) => {
    const norm = p.replaceAll("\\", "/");
    const idx = norm.lastIndexOf("/");
    return idx > 0 ? norm.slice(0, idx) : "";
  };

  async function copyHash(hash: string) {
    await copyText(hash);
  }

  // dev-only automation seam (demo/screenshot harness): the same real code
  // path the dialogs and drop handler take, reachable from CDP without the
  // native file dialog. Stripped from release builds.
  if (import.meta.env.DEV) {
    (window as unknown as Record<string, unknown>).__todokeSend = {
      add: (paths: string[]) => {
        if (phase === "pick") addPaths(paths);
      },
      start: () => void startSend(),
    };
  }
</script>

{#snippet pageHead(title: string, sub: string)}
  <header class="page-head">
    <div>
      <h1>{title}</h1>
      <p class="page-sub">{sub}</p>
    </div>
    {#if phase === "sending"}
      <button type="button" class="btn btn-danger" onclick={() => void cancelSend()}>Cancel transfer</button>
    {:else if phase === "receipt" || phase === "failed"}
      <button type="button" class="btn btn-ghost" onclick={reset}>Send something else</button>
    {/if}
  </header>
{/snippet}

{#if phase === "pick"}
  {@render pageHead("Send", "Pick what moves and where it arrives.")}

  <div class="dropzone" class:over={dragOver} aria-label="Selected files and folders">
    {#if items.length === 0}
      <div class="pick-empty">
        <span class="ico ico-40">{@html icons.upload}</span>
        <p class="empty-line">Nothing selected yet.</p>
      </div>
    {/if}

    {#if items.length > 0}
      <ul class="items" role="list">
        {#each items as p (p)}
          <li>
            <span class="fname">{fileName(p)}</span>
            <span class="fdir">{dirName(p)}</span>
            <button
              type="button"
              class="x"
              aria-label={`Remove ${fileName(p)}`}
              onclick={() => removePath(p)}
            >×</button>
          </li>
        {/each}
      </ul>
      <p class="tally">
        {statting ? "counting…" : stats
          ? `${stats.files} file${stats.files === 1 ? "" : "s"} · ${formatBytes(stats.totalBytes)}${stats.skipped > 0 ? ` · ${stats.skipped} unreadable` : ""}`
          : "ready"}
      </p>
    {/if}

    <div class="pick-actions">
      <button type="button" class="btn btn-primary" onclick={() => void addFiles()}>Add files…</button>
      <button type="button" class="btn btn-ghost" onclick={() => void addFolder()}>Add folder…</button>
      {#if items.length > 0}
        <button type="button" class="btn btn-ghost" onclick={clearItems}>Clear</button>
      {/if}
      <span class="hint">or drop files anywhere in this window</span>
    </div>
  </div>

  <section class="dest">
    <h2>Destination</h2>
    {#if devices.length === 0}
      <p class="empty-line">
        No devices yet — keep this open, and check the
        <button type="button" class="link" onclick={() => go("devices")}>Devices</button>
        screen.
      </p>
    {:else}
      <div class="devlist" role="radiogroup" aria-label="Destination device">
        {#each devices as d (d.fingerprint)}
          <button
            type="button"
            class="dev"
            class:picked={selectedFp === d.fingerprint}
            role="radio"
            aria-checked={selectedFp === d.fingerprint}
            onclick={() => (selectedFp = d.fingerprint)}
          >
            <span class="dname">{d.name}</span>
            <span class="dfp">{shortFp(d.fingerprint)}</span>
          </button>
        {/each}
      </div>
      <button
        type="button"
        class="btn btn-primary btn-lg send"
        disabled={!selectedDevice || items.length === 0}
        onclick={() => void startSend()}
      >
        Send {items.length > 0 && stats ? `${stats.files} file${stats.files === 1 ? "" : "s"}` : ""} {selectedDevice ? `to ${selectedDevice.name}` : ""}
      </button>
    {/if}
  </section>
{:else if phase === "sending"}
  {@render pageHead(
    "Sending",
    selectedDevice ? `to ${selectedDevice.name} (${shortFp(selectedDevice.fingerprint)})` : "to your device",
  )}

  <div class="card progress-card">
    <div class="big-row">
      <span class="pct">{Math.round(fraction * 100)}%</span>
      <span class="meta">
        {formatBytes(sentBytes)} / {formatBytes(totalBytes)}
        · {formatSpeed(speed)} · ETA {etaString(remaining, speed)}
      </span>
    </div>
    <div
      class="bar"
      role="progressbar"
      aria-valuenow={Math.round(fraction * 100)}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <div class="fill" style={`transform: scaleX(${fraction})`}></div>
    </div>

    <ul class="inflight" role="list">
      {#each activeFiles as [path, f] (path)}
        <li>
          <span class="fname">{fileName(path)}</span>
          <span class="fbar"><span class="ffill" style={`transform: scaleX(${f.size > 0 ? f.sent / f.size : 0})`}></span></span>
          <span class="fmeta">{formatBytes(f.sent)} / {formatBytes(f.size)}</span>
        </li>
      {/each}
    </ul>
    <p class="tally">{doneCount} file{doneCount === 1 ? "" : "s"} finalized and verifying as they land</p>
  </div>
{:else if phase === "receipt" && receipt}
  {@render pageHead("Arrived ✓", "Every byte verified end-to-end.")}

  <div class="card receipt">
    <p class="r-sub">
      {receipt.files.length} file{receipt.files.length === 1 ? "" : "s"} ·
      {formatBytes(receipt.files.reduce((a, f) => a + f.size, 0))} ·
      manifest <span class="mono">{shortFp(receipt.manifestHash)}</span>
    </p>
    <ul class="r-files" role="list">
      {#each receipt.files as f, i (f.path)}
        <li style={`--i: ${Math.min(i, 8)}`}>
          <span class="fname">{fileName(f.path)}</span>
          <span class="fmeta">{formatBytes(f.size)}</span>
          <button
            type="button"
            class="hash"
            title={`Copy hash ${f.hash}`}
            onclick={() => void copyHash(f.hash)}
          >
            <span class="mono">{f.hash.slice(0, 16)}…</span> copy
          </button>
        </li>
      {/each}
    </ul>
  </div>
{:else if phase === "failed"}
  {@render pageHead("Didn't arrive", cancelledByUser ? "You stopped the transfer." : "The transfer ran into a problem.")}

  <div class="failed">
    <p class="f-msg">{failMessage}</p>
    <p class="f-note">
      {cancelledByUser
        ? "The receiver kept everything that already landed — send again and it resumes where this stopped."
        : "Everything received before the failure is kept on the receiver; sending the same files again resumes where this stopped."}
    </p>
  </div>
{/if}

{#if dragOver && phase === "pick"}
  <div class="overlay" aria-hidden="true">
    <span>drop to add to the lane</span>
  </div>
{/if}

<style>
  h2 {
    font-size: var(--fs-section);
    margin-bottom: 10px;
  }

  .hint {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }

  .empty-line {
    color: var(--text-2);
  }

  /* Dropzone: dashed hairline card, accent on hover/drag */
  .dropzone {
    border: 1.5px dashed var(--hairline);
    border-radius: var(--r-card);
    background: var(--surface);
    box-shadow: var(--shadow-card);
    padding: 20px;
    transition:
      border-color var(--dur-1) var(--ease-out),
      background var(--dur-1) var(--ease-out);
  }

  .dropzone.over {
    border-color: var(--accent);
    background: color-mix(in oklab, var(--accent) 4%, var(--surface));
  }

  .pick-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 8px 0 4px;
  }

  .pick-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    margin-top: 16px;
  }

  .items {
    list-style: none;
    margin: 16px 0 0;
    padding: 0;
    max-height: 16rem;
    overflow-y: auto;
  }

  .items li {
    display: grid;
    grid-template-columns: minmax(10rem, 1fr) minmax(0, 2fr) auto;
    align-items: baseline;
    gap: 12px;
    padding: 8px 0;
    border-top: 1px solid var(--sep);
  }

  .items li:first-child {
    border-top: none;
  }

  .fname {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fdir {
    color: var(--text-3);
    font-size: var(--fs-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
  }

  .x {
    color: var(--text-3);
    font-size: 15px;
    line-height: 1;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    display: grid;
    place-items: center;
    transition:
      color var(--dur-1) var(--ease-out),
      background var(--dur-1) var(--ease-out),
      transform var(--dur-1) var(--ease-out);
  }

  .x:hover {
    color: var(--err);
    background: color-mix(in oklab, var(--err) 10%, transparent);
  }

  .tally {
    color: var(--text-2);
    font-size: var(--fs-sm);
    margin-top: 12px;
  }

  .dest {
    margin-top: 28px;
  }

  .devlist {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .dev {
    display: flex;
    flex-direction: column;
    gap: 3px;
    align-items: flex-start;
    border: 1px solid var(--hairline);
    background: var(--surface);
    border-radius: 10px;
    padding: 10px 14px;
    transition:
      border-color var(--dur-1) var(--ease-out),
      background var(--dur-1) var(--ease-out),
      transform var(--dur-1) var(--ease-out);
  }

  .dev:hover {
    background: var(--fill);
  }

  .dev.picked {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .dname {
    color: var(--text);
    font-weight: 600;
  }

  .dfp {
    color: var(--text-2);
    font-size: var(--fs-xs);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }

  .send {
    margin-top: 20px;
  }

  .link {
    color: var(--accent);
    font-weight: 500;
  }

  .link:hover {
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  /* Sending */
  .progress-card {
    max-width: 46rem;
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 20px;
  }

  .big-row {
    display: flex;
    align-items: baseline;
    gap: 14px;
    flex-wrap: wrap;
  }

  .pct {
    color: var(--text);
    font-size: 38px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    letter-spacing: -0.02em;
    line-height: 1;
  }

  .meta {
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }

  .inflight {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .inflight li {
    display: grid;
    grid-template-columns: minmax(8rem, 1fr) 2fr minmax(6rem, auto);
    gap: 12px;
    align-items: center;
    font-size: var(--fs-sm);
  }

  .fbar {
    height: 4px;
    background: var(--track);
    border-radius: var(--r-pill);
    overflow: hidden;
  }

  .ffill {
    display: block;
    height: 100%;
    background: var(--accent);
    transform-origin: 0 50%;
    border-radius: inherit;
    transition: transform var(--dur-2) var(--ease-out);
  }

  .fmeta {
    color: var(--text-2);
    font-family: var(--mono);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  /* Receipt */
  .receipt {
    max-width: 46rem;
    padding: 20px;
  }

  .receipt .r-sub {
    color: var(--text-2);
    font-size: var(--fs-sm);
    margin-bottom: 12px;
  }

  .r-files {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .r-files li {
    display: grid;
    grid-template-columns: minmax(10rem, 2fr) minmax(5rem, auto) auto;
    gap: 12px;
    align-items: baseline;
    padding: 10px 0;
    border-top: 1px solid var(--sep);
    animation: rise var(--dur-2) var(--ease-out) both;
    animation-delay: calc(var(--i) * 40ms);
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }

  .hash {
    color: var(--text-2);
    font-size: 11.5px;
    border-radius: 6px;
    padding: 3px 8px;
    transition:
      color var(--dur-1) var(--ease-out),
      background var(--dur-1) var(--ease-out),
      transform var(--dur-1) var(--ease-out);
  }

  .hash:hover {
    color: var(--accent);
    background: var(--accent-soft);
  }

  /* Failed */
  .failed {
    max-width: 46rem;
    display: flex;
    flex-direction: column;
    gap: 10px;
    background: color-mix(in oklab, var(--err) 6%, var(--surface));
    border-radius: var(--r-card);
    box-shadow: var(--shadow-card);
    padding: 20px;
  }

  .f-msg {
    color: var(--text);
    user-select: text;
  }

  .f-note {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }

  /* Drag overlay */
  .overlay {
    position: fixed;
    inset: 12px;
    z-index: 10;
    display: grid;
    place-items: center;
    background: color-mix(in oklab, var(--bg) 55%, transparent);
    backdrop-filter: blur(6px);
    -webkit-backdrop-filter: blur(6px);
    border: 2px dashed var(--accent);
    border-radius: var(--r-hero);
    color: var(--accent);
    font-size: var(--fs-section);
    font-weight: 600;
    pointer-events: none;
  }
</style>
