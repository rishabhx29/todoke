<script lang="ts">
  import { listDevices, type Device } from "../api";
  import { app, go } from "../state.svelte";
  import { icons } from "../icons";
  import { shortFp } from "../format";

  let devices = $state<Device[]>([]);
  let error = $state<string | null>(null);
  let scanning = $state(false);
  let scanningSince = $state<number | null>(null);
  let showHelp = $state(false);

  let timer: ReturnType<typeof setTimeout> | undefined;
  let alive = true;

  async function scan() {
    scanning = true;
    scanningSince = Date.now();
    try {
      devices = await listDevices(2500);
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
  <button type="button" class="btn btn-ghost" onclick={() => void scan()} disabled={scanning}>
    {scanning ? "Scanning…" : "Refresh"}
  </button>
</header>

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
          {/if}
          <span class="fp">{shortFp(d.fingerprint)} · {d.addr}:{d.port}</span>
        </div>
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
</style>
