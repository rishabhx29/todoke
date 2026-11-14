<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { receiveStart, receiveStop, setReceiveTarget } from "../api";
  import { app } from "../state.svelte";
  import { icons } from "../icons";
  import { formatBytes, timeAgo } from "../format";

  let busy = $state(false);
  let error = $state<string | null>(null);
  let expanded = $state<string | null>(null);

  async function toggle() {
    if (busy) return;
    busy = true;
    error = null;
    try {
      app.receive = app.receive.on ? await receiveStop() : await receiveStart();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function changeTarget() {
    const dir = await open({
      directory: true,
      multiple: false,
      title: "Where should received files land?",
      defaultPath: app.receive.target ?? undefined,
    });
    if (typeof dir !== "string") return;
    busy = true;
    error = null;
    try {
      app.receive = await setReceiveTarget(dir);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function toggleExpand(hash: string) {
    expanded = expanded === hash ? null : hash;
  }
</script>

<header class="page-head">
  <div>
    <h1>Receive</h1>
    <p class="page-sub">
      When receive is on, other devices can send files to this one — it runs
      in the background of every screen.
    </p>
  </div>
</header>

<section class="card panel">
  <div class="row switch-row">
    <div class="switch-copy">
      <span class="switch-title">
        {app.receive.on ? "Receiving" : "Receive is off"}
        {#if app.receive.on}
          <span class="live" aria-hidden="true"></span>
        {/if}
      </span>
      <span class="switch-sub">
        {#if app.receive.on}
          as {app.identity?.name ?? "this device"} · files land in {app.receive.target}
        {:else}
          flip it on to appear on other devices' screens
        {/if}
      </span>
    </div>
    <button
      type="button"
      class="switch"
      class:on={app.receive.on}
      role="switch"
      aria-checked={app.receive.on}
      aria-label="Receive mode"
      disabled={busy}
      onclick={() => void toggle()}
    >
      <span class="knob"></span>
    </button>
  </div>

  <div class="row target-row">
    <span class="target-label">
      <span class="ico ico-16">{@html icons.folder}</span>
      Target folder
    </span>
    <span class="target-path mono" title={app.receive.target ?? ""}>
      {app.receive.target ?? app.identity?.defaultTarget ?? "—"}
    </span>
    <button
      type="button"
      class="btn btn-ghost"
      onclick={() => void changeTarget()}
      disabled={busy || app.receive.on}
      title={app.receive.on ? "Stop receive mode before changing the target folder" : undefined}
    >
      Change…
    </button>
  </div>

  {#if error}
    <p class="form-error" role="alert">{error}</p>
  {/if}
</section>

<section class="log">
  <div class="log-head">
    <h2>Incoming transfers</h2>
    {#if app.incoming.length > 0}
      <span class="pill">{app.incoming.length}</span>
    {/if}
  </div>

  {#if app.incoming.length === 0}
    <div class="empty-state">
      <span class="ico ico-40">{@html icons.inbox}</span>
      <p class="empty-line">Nothing has arrived yet.</p>
    </div>
  {:else}
    <div class="card">
      <ul class="entries" role="list">
        {#each app.incoming as r (r.atUnixMs + r.manifestHash)}
          <li class:bad={!r.ok}>
            <button
              type="button"
              class="entry"
              aria-expanded={expanded === r.manifestHash + r.atUnixMs}
              onclick={() => toggleExpand(r.manifestHash + r.atUnixMs)}
            >
              <span class="mark" class:ok={r.ok} aria-hidden="true">
                <span class="ico ico-16">{@html r.ok ? icons.check : icons.alert}</span>
              </span>
              <span class="who">{r.fromAddr}</span>
              <span class="what">
                {r.ok
                  ? `${r.files.length} file${r.files.length === 1 ? "" : "s"} · ${formatBytes(r.totalBytes)}`
                  : "failed"}
              </span>
              <span class="when">{timeAgo(r.atUnixMs)}</span>
            </button>
            {#if expanded === r.manifestHash + r.atUnixMs}
              <div class="detail">
                {#if r.ok}
                  <ul role="list">
                    {#each r.files as f (f.path)}
                      <li>
                        <span class="d-path">{f.path}</span>
                        <span class="d-meta">{formatBytes(f.size)} · <span class="mono d-hash">{f.hash.slice(0, 12)}…</span></span>
                      </li>
                    {/each}
                  </ul>
                {:else}
                  <p class="d-err">{r.error}</p>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    </div>
  {/if}
</section>

<style>
  .panel {
    max-width: 46rem;
    padding: 4px 16px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
  }

  .switch-row {
    justify-content: space-between;
  }

  .target-row {
    border-top: 1px solid var(--sep);
  }

  .switch-copy {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .switch-title {
    color: var(--text);
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  /* Listening dot: 8px green core, ring grows 8px -> 10px while fading, 2s ease */
  .live {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--ok);
    position: relative;
  }

  .live::after {
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
      transform: scale(1.25);
      opacity: 0;
    }
  }

  .switch-sub {
    color: var(--text-2);
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Apple toggle */
  .switch {
    width: 40px;
    height: 24px;
    flex: none;
    border-radius: var(--r-pill);
    background: var(--fill);
    box-shadow: inset 0 0 0 1px var(--hairline);
    position: relative;
    transition:
      background var(--dur-2) var(--ease-out),
      box-shadow var(--dur-2) var(--ease-out),
      transform var(--dur-1) var(--ease-out);
  }

  .switch .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: #ffffff;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
    transition:
      transform var(--dur-2) var(--ease-out),
      background var(--dur-2) var(--ease-out);
  }

  .switch.on {
    background: var(--accent);
    box-shadow: none;
  }

  .switch.on .knob {
    transform: translateX(16px);
    background: var(--on-accent);
  }

  .switch:disabled {
    opacity: 0.55;
  }

  .target-label {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
    font-size: var(--fs-body);
    flex: none;
  }

  .target-path {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: right;
  }

  .log {
    margin-top: 28px;
    max-width: 46rem;
  }

  .log-head {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 10px;
  }

  .log-head h2 {
    font-size: var(--fs-section);
  }

  .empty-line {
    color: var(--text-2);
  }

  .entries {
    list-style: none;
    margin: 0;
    padding: 0 16px;
  }

  .entry {
    width: 100%;
    display: grid;
    grid-template-columns: auto minmax(8rem, 1fr) minmax(0, 1fr) auto;
    align-items: center;
    gap: 12px;
    min-height: 44px;
    padding: 8px 0;
    text-align: left;
    transition: background var(--dur-1) var(--ease-out), transform var(--dur-1) var(--ease-out);
  }

  .entry:hover {
    background: var(--fill);
    border-radius: var(--r-btn);
  }

  .mark {
    width: 22px;
    height: 22px;
    display: grid;
    place-items: center;
    border-radius: 50%;
    color: var(--ok);
    background: color-mix(in oklab, var(--ok) 14%, transparent);
  }

  li.bad .mark {
    color: var(--err);
    background: color-mix(in oklab, var(--err) 14%, transparent);
  }

  .who {
    color: var(--text);
    font-weight: 500;
    font-family: var(--mono);
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .what {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }

  .when {
    color: var(--text-3);
    font-size: var(--fs-xs);
  }

  .detail {
    padding: 0 0 12px calc(22px + 12px);
    border-top: 1px solid var(--sep);
    margin-top: -4px;
  }

  .detail ul {
    list-style: none;
    margin: 10px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .detail li {
    display: flex;
    justify-content: space-between;
    gap: 16px;
  }

  .d-path {
    color: var(--text);
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  .d-meta {
    color: var(--text-2);
    font-size: 11.5px;
    font-family: var(--mono);
    flex: none;
    font-variant-numeric: tabular-nums;
    user-select: text;
  }

  .d-err {
    color: var(--err);
    font-size: var(--fs-sm);
    padding-top: 10px;
    user-select: text;
  }
</style>
