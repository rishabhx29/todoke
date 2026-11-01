<script lang="ts">
  import { listDevices, type Device } from "../api";
  import { app, go } from "../state.svelte";
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

<header class="head">
  <div>
    <h1>Devices</h1>
    <p class="sub">
      {#if scanningSince}
        listening for Todoke devices on this network…
      {:else}
        {devices.length} device{devices.length === 1 ? "" : "s"} found ·
        refreshes every few seconds
      {/if}
    </p>
  </div>
  <button type="button" class="ghost" onclick={() => void scan()} disabled={scanning}>
    {scanning ? "Scanning…" : "Refresh"}
  </button>
</header>

{#if error}
  <p class="error" role="alert">Discovery failed: {error}</p>
{/if}

{#if devices.length > 0}
  <ul class="list" role="list">
    {#each devices as d, i (d.fingerprint)}
      <li class="row" style={`--i: ${Math.min(i, 8)}`}>
        <span class="pulse" aria-hidden="true"></span>
        <div class="who">
          <span class="name">{d.name}</span>
          <span class="fp">{shortFp(d.fingerprint)} · {d.addr}:{d.port}</span>
        </div>
        <button type="button" class="cta" onclick={() => sendTo(d)}>Send…</button>
      </li>
    {/each}
  </ul>
{:else if !scanningSince}
  <div class="empty">
    <h2>Nothing on the lane yet</h2>
    <p>
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
    <button type="button" class="ghost" aria-expanded={showHelp} onclick={() => (showHelp = !showHelp)}>
      {showHelp ? "Hide" : "Show"} same-machine & firewall notes
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
  .head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--s4);
    margin-bottom: var(--s5);
  }

  h1 {
    font-size: var(--fs-h1);
  }

  .sub {
    color: var(--text-dim);
    font-size: var(--fs-sm);
    margin-top: var(--s1);
  }

  .error {
    color: var(--err);
    font-size: var(--fs-sm);
    margin-bottom: var(--s4);
  }

  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .row {
    display: flex;
    align-items: center;
    gap: var(--s4);
    padding: var(--s4) var(--s2);
    border-top: 1.5rem solid transparent;
    border-image: var(--lane-dash) 1;
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

  .pulse {
    width: 9px;
    height: 9px;
    flex: none;
    border-radius: 50%;
    background: var(--volt);
    animation: breathe 2.4s ease-in-out infinite;
  }

  @keyframes breathe {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.35;
    }
  }

  .who {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .name {
    color: var(--text-hi);
    font-weight: 620;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .fp {
    color: var(--text-dim);
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
  }

  .cta {
    background: var(--volt);
    color: var(--volt-ink);
    font-weight: 640;
    padding: var(--s2) var(--s4);
    border-radius: var(--r-pill);
    transition: transform var(--dur-1) var(--ease-out);
  }

  .cta:hover {
    transform: translateY(-1px);
  }

  .empty {
    border: 1px dashed var(--line);
    border-radius: var(--r-tile);
    padding: var(--s6);
    display: flex;
    flex-direction: column;
    gap: var(--s4);
    max-width: 46rem;
  }

  .empty h2 {
    font-size: var(--fs-h2);
  }

  .empty ol {
    margin: 0;
    padding-left: var(--s5);
    display: flex;
    flex-direction: column;
    gap: var(--s2);
  }

  .ghost {
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: var(--r-pill);
    padding: var(--s2) var(--s4);
    transition:
      border-color var(--dur-1) var(--ease-out),
      color var(--dur-1) var(--ease-out);
  }

  .ghost:hover:not(:disabled) {
    color: var(--volt);
    border-color: var(--volt);
  }

  .ghost:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .help {
    display: flex;
    flex-direction: column;
    gap: var(--s2);
    color: var(--text-dim);
    font-size: var(--fs-sm);
    border-top: 1px dashed var(--line);
    padding-top: var(--s4);
  }

  .help code {
    display: block;
    background: var(--ink-1);
    border: 1px solid var(--line);
    border-radius: var(--r-tile);
    padding: var(--s2) var(--s3);
    color: var(--text);
    font-size: var(--fs-xs);
    overflow-x: auto;
    user-select: text;
  }
</style>
