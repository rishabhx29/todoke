<script lang="ts">
  import { listDevices, type Device } from "../api";
  import { app, addManualPeer, removeManualPeer, withManualPeers, go } from "../state.svelte";
  import { icons } from "../icons";
  import { shortFp } from "../format";

  let devices = $state<Device[]>([]);
  let error = $state<string | null>(null);
  let scanning = $state(false);
  let scanningSince = $state<number | null>(null);
  let showHelp = $state(false);
  let showAdd = $state(false);
  let form = $state({ name: "", addr: "", fingerprint: "", certB64: "" });
  let formError = $state<string | null>(null);

  function saveManual() {
    const addr = form.addr.trim();
    const fingerprint = form.fingerprint.trim().toUpperCase();
    const certB64 = form.certB64.trim();
    if (!/^\S+:\d+$/.test(addr)) {
      formError = "Address looks like host:port — e.g. 192.168.1.24:53411";
      return;
    }
    if (fingerprint.length < 8) {
      formError = "Paste the full device fingerprint (Settings → Fingerprint on the other machine).";
      return;
    }
    if (certB64.length < 40) {
      formError = "Paste the full certificate (the long base64 line from `todoke serve --json`).";
      return;
    }
    addManualPeer({
      name: form.name.trim() || `device-${fingerprint.slice(0, 4).toLowerCase()}`,
      addr, fingerprint, certB64,
    });
    form = { name: "", addr: "", fingerprint: "", certB64: "" };
    formError = null;
    showAdd = false;
    devices = withManualPeers(devices);
  }

  let timer: ReturnType<typeof setTimeout> | undefined;
  let alive = true;

  async function scan() {
    scanning = true;
    scanningSince = Date.now();
    try {
      devices = withManualPeers(await listDevices(2500));
      error = null;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      scanning = false;
      scanningSince = null;
      if (alive) timer = setTimeout(scan, 3000);
    }
  }

  $effect(() => {
    alive = true;
    void scan();
    return () => {
      alive = false;
      if (timer) clearTimeout(timer);
    };
  });

  function sendTo(d: Device) {
    app.pendingDestination = d.fingerprint;
    go("send");
  }
</script>

<header class="page-head">
  <div>
    <h1>Devices</h1>
    <p class="page-sub">
      {#if scanningSince}
        Listening for Todoke devices on this network…
      {:else}
        {devices.length} device{devices.length === 1 ? "" : "s"} found ·
        refreshes every few seconds
      {/if}
    </p>
  </div>
  <div class="head-actions">
    <button type="button" class="btn btn-ghost" class:active={showAdd} onclick={() => (showAdd = !showAdd)} aria-expanded={showAdd}>
      {showAdd ? "Close" : "Add by address"}
    </button>
    <button type="button" class="btn btn-ghost" onclick={() => void scan()} disabled={scanning}>
      {scanning ? "Scanning…" : "Refresh"}
    </button>
  </div>
</header>

{#if showAdd}
  <section class="card add-card" aria-label="Add a device by address">
    <p class="add-lede">
      For networks that block discovery (campus and public Wi-Fi often do):
      run <code>todoke serve --json</code> on the other machine and paste the
      address, fingerprint and certificate it prints.
    </p>
    <div class="add-grid">
      <label><span>Name</span><input bind:value={form.name} placeholder="studio-laptop (optional)" /></label>
      <label><span>Address</span><input bind:value={form.addr} placeholder="192.168.1.24:53411" /></label>
      <label><span>Fingerprint</span><input bind:value={form.fingerprint} placeholder="2C83…" /></label>
      <label class="wide"><span>Certificate (base64)</span><input bind:value={form.certB64} placeholder="MIIBYDCCARKgAwIBAgIJ…" /></label>
    </div>
    {#if formError}<p class="form-error" role="alert">{formError}</p>{/if}
    <div class="add-actions">
      <button type="button" class="btn btn-primary" onclick={saveManual}>Save device</button>
    </div>
  </section>
{/if}

{#if error}
  <p class="form-error" role="alert">Discovery failed: {error}</p>
{/if}

{#if devices.length > 0}
  <ul class="list" role="list">
    {#each devices as d, i (d.fingerprint)}
      <li class="card row" style={`--i: ${Math.min(i, 8)}`}>
        <span class="presence" aria-hidden="true"></span>
        <div class="who">
          <span class="name">{d.name}</span>
          {#if d.fingerprint === app.identity?.fingerprint}
            <span class="pill pill-accent">This device</span>
          {:else if d.manual}
            <span class="pill">saved</span>
          {/if}
          <span class="fp">{shortFp(d.fingerprint)} · {d.addr}:{d.port}</span>
        </div>
        {#if d.manual}
          <button type="button" class="btn btn-ghost btn-remove" aria-label={`Remove ${d.name}`} onclick={() => removeManualPeer(d.fingerprint)}>×</button>
        {/if}
        <button type="button" class="btn btn-primary" onclick={() => sendTo(d)}>Send…</button>
      </li>
    {/each}
  </ul>
{:else if !scanningSince}
  <div class="empty-state">
    <span class="ico ico-40">{@html icons.devicesSearch}</span>
    <h2>Nothing on the lane yet</h2>
    <p class="lede">
      Todoke finds other devices on the same network automatically. To see
      something here:
    </p>
    <ol>
      <li>
        Install Todoke (or run <code>todoke serve</code> from the
        command-line tool) on a second device on this network.
      </li>
      <li>
        Keep receive mode on there — it appears here within a few seconds.
      </li>
    </ol>
    <button type="button" class="btn btn-ghost" aria-expanded={showHelp} onclick={() => (showHelp = !showHelp)}>
      {showHelp ? "Hide" : "Show"} same-machine &amp; firewall notes
    </button>
    {#if showHelp}
      <div class="help">
        <p>
          Testing on this machine only: start the receiver with loopback
          advertising — Windows lets unsigned apps use mDNS there:
        </p>
        <code>todoke serve --target ./incoming --advertise-ip 127.0.0.1</code>
        <p>
          On a real network, one-time admin PowerShell (unsigned binaries get
          their mDNS silently dropped until then):
        </p>
        <code>
          netsh advfirewall firewall add rule name="Todoke" dir=in action=allow
          protocol=UDP localport=5353
        </code>
      </div>
    {/if}
  </div>
{/if}

<style>
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 46rem;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 14px 16px;
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

  .presence {
    width: 8px;
    height: 8px;
    flex: none;
    border-radius: 50%;
    background: var(--ok);
    position: relative;
  }

  .presence::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: var(--ok);
    animation: pulse 2s ease infinite;
  }

  @keyframes pulse {
    from {
      transform: scale(1);
      opacity: 0.45;
    }
    to {
      transform: scale(1.6);
      opacity: 0;
    }
  }

  .who {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
    min-width: 0;
    flex: 1;
  }

  .name {
    color: var(--text);
    font-size: 14px;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 8px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fp {
    color: var(--text-2);
    font-size: var(--fs-xs);
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty-state h2 {
    font-size: var(--fs-section);
  }

  .lede {
    color: var(--text-2);
    max-width: 28rem;
  }

  ol {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    color: var(--text-2);
    font-size: var(--fs-sm);
    text-align: left;
    max-width: 30rem;
  }

  ol code {
    font-family: var(--mono);
    font-size: var(--fs-xs);
  }

  .help {
    display: flex;
    flex-direction: column;
    gap: 10px;
    color: var(--text-2);
    font-size: var(--fs-sm);
    border-top: 1px solid var(--sep);
    padding-top: 16px;
    max-width: 30rem;
  }

  .help code {
    display: block;
    background: var(--inset);
    border-radius: var(--r-input);
    padding: 8px 12px;
    color: var(--text);
    font-size: var(--fs-xs);
    font-family: var(--mono);
    overflow-x: auto;
    user-select: text;
    text-align: left;
  }

  .head-actions {
    display: flex;
    gap: 10px;
  }

  .btn-ghost.active {
    background: var(--accent-soft);
    color: var(--accent);
    border-color: var(--accent-line);
  }

  .add-card {
    margin: 0 0 18px;
    padding: 20px 22px;
    display: grid;
    gap: 14px;
  }

  .add-lede {
    margin: 0;
    font-size: var(--fs-sm);
    color: var(--text-2);
    max-width: 42rem;
  }

  .add-lede code {
    font-family: var(--mono);
    font-size: var(--fs-xs);
    background: var(--inset);
    border-radius: 6px;
    padding: 2px 6px;
    color: var(--text);
    user-select: text;
  }

  .add-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }

  .add-grid label {
    display: grid;
    gap: 5px;
    font-size: var(--fs-xs);
    color: var(--text-2);
  }

  .add-grid label.wide {
    grid-column: 1 / -1;
  }

  .add-grid input {
    font-family: var(--mono);
    font-size: var(--fs-sm);
    color: var(--text);
    background: var(--inset);
    border: 1px solid var(--hairline);
    border-radius: var(--r-input);
    padding: 9px 12px;
    outline: none;
    user-select: text;
  }

  .add-grid input:focus-visible {
    outline: none;
    border-color: var(--accent-line);
    box-shadow: 0 0 0 2px var(--accent-soft);
  }

  .add-grid input::placeholder {
    color: var(--text-3);
  }

  .add-actions {
    display: flex;
    justify-content: flex-end;
  }

  .btn-remove {
    min-width: 32px;
    padding-inline: 8px;
    font-size: 16px;
  }
</style>
