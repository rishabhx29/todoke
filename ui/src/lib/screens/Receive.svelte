<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { receiveStart, receiveStop, setReceiveTarget } from "../api";
  import { app } from "../state.svelte";
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

<header class="head">
  <div>
    <h1>Receive</h1>
    <p class="sub">
      When receive is on, other devices can send files to this one — it runs
      in the background of every screen.
    </p>
  </div>
</header>

<section class="panel">
  <div class="switch-row">
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

  <div class="target-row">
    <span class="target-label">Target folder</span>
    <span class="target-path" title={app.receive.target ?? ""}>
      {app.receive.target ?? app.identity?.defaultTarget ?? "—"}
    </span>
    <button
      type="button"
      class="ghost"
      onclick={() => void changeTarget()}
      disabled={busy || app.receive.on}
      title={app.receive.on ? "Stop receive mode before changing the target folder" : undefined}
    >
      Change…
    </button>
  </div>

  {#if error}
    <p class="error" role="alert">{error}</p>
  {/if}
</section>

<section class="log">
  <div class="log-head">
    <h2>Incoming transfers</h2>
    {#if app.incoming.length > 0}
      <span class="count">{app.incoming.length}</span>
    {/if}
  </div>

  {#if app.incoming.length === 0}
    <p class="empty-line">Nothing has arrived yet.</p>
  {:else}
    <ul class="entries" role="list">
      {#each app.incoming as r (r.atUnixMs + r.manifestHash)}
        <li class:bad={!r.ok}>
          <button
            type="button"
            class="entry"
            aria-expanded={expanded === r.manifestHash + r.atUnixMs}
            onclick={() => toggleExpand(r.manifestHash + r.atUnixMs)}
          >
            <span class="mark" class:ok={r.ok} aria-hidden="true">{r.ok ? "✓" : "!"}</span>
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
                      <span class="d-meta">{formatBytes(f.size)} · <span class="mono">{f.hash.slice(0, 12)}…</span></span>
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
  {/if}
</section>

<style>
  .head {
    margin-bottom: var(--s5);
  }

  h1 {
    font-size: var(--fs-h1);
  }

  h2 {
    font-size: var(--fs-h3);
  }

  .sub {
    color: var(--text-dim);
    font-size: var(--fs-sm);
    margin-top: var(--s1);
    max-width: 44rem;
  }

  .panel {
    border: 1px dashed var(--line);
    border-radius: var(--r-tile);
    padding: var(--s5);
    display: flex;
    flex-direction: column;
    gap: var(--s4);
    max-width: 46rem;
  }

  .switch-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--s4);
  }

  .switch-copy {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .switch-title {
    color: var(--text-hi);
    font-weight: 640;
    display: flex;
    align-items: center;
    gap: var(--s2);
  }

  .live {
    width: 8px;
    height: 8px;
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

  .switch-sub {
    color: var(--text-dim);
    font-size: var(--fs-sm);
  }

  .switch {
    width: 52px;
    height: 28px;
    flex: none;
    border-radius: var(--r-pill);
    background: var(--ink-2);
    border: 1px solid var(--line);
    position: relative;
    transition:
      background var(--dur-2) var(--ease-out),
      border-color var(--dur-2) var(--ease-out);
  }

  .switch .knob {
    position: absolute;
    top: 3px;
    left: 3px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--text-dim);
    transition:
      transform var(--dur-2) var(--ease-out),
      background var(--dur-2) var(--ease-out);
  }

  .switch.on {
    background: color-mix(in oklab, var(--volt) 22%, var(--ink-2));
    border-color: var(--volt);
  }

  .switch.on .knob {
    transform: translateX(24px);
    background: var(--volt);
  }

  .switch:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .target-row {
    display: grid;
    grid-template-columns: auto 1fr auto;
    align-items: baseline;
    gap: var(--s3);
    border-top: 1px dashed var(--line);
    padding-top: var(--s4);
  }

  .target-label {
    color: var(--text-dim);
    font-size: var(--fs-sm);
  }

  .target-path {
    color: var(--text);
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
  }

  .ghost {
    color: var(--text);
    border: 1px solid var(--line);
    border-radius: var(--r-pill);
    padding: var(--s1) var(--s3);
    font-size: var(--fs-sm);
    transition:
      border-color var(--dur-1) var(--ease-out),
      color var(--dur-1) var(--ease-out);
  }

  .ghost:hover:not(:disabled) {
    color: var(--volt);
    border-color: var(--volt);
  }

  .ghost:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .error {
    color: var(--err);
    font-size: var(--fs-sm);
    user-select: text;
  }

  .log {
    margin-top: var(--s6);
  }

  .log-head {
    display: flex;
    align-items: baseline;
    gap: var(--s3);
    margin-bottom: var(--s3);
  }

  .count {
    color: var(--volt);
    font-size: var(--fs-sm);
    font-variant-numeric: tabular-nums;
  }

  .empty-line {
    color: var(--text-dim);
  }

  .entries {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .entries li {
    border-top: 1px dashed var(--line);
  }

  .entry {
    width: 100%;
    display: grid;
    grid-template-columns: auto minmax(8rem, 1fr) minmax(0, 1fr) auto;
    align-items: baseline;
    gap: var(--s3);
    padding: var(--s3) var(--s1);
    text-align: left;
  }

  .entry:hover {
    background: var(--ink-1);
  }

  .mark {
    width: 1.4rem;
    height: 1.4rem;
    display: grid;
    place-items: center;
    border-radius: 50%;
    font-size: 0.8rem;
    align-self: center;
  }

  .mark.ok {
    color: var(--volt-ink);
    background: var(--volt);
  }

  li.bad .mark {
    color: var(--err);
    background: color-mix(in oklab, var(--err) 15%, transparent);
  }

  .who {
    color: var(--text-hi);
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }

  .what {
    color: var(--text-dim);
    font-size: var(--fs-sm);
  }

  .when {
    color: var(--text-dim);
    font-size: var(--fs-xs);
  }

  .detail {
    padding: 0 var(--s1) var(--s4) calc(1.4rem + var(--s3) + var(--s1));
  }

  .detail ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: var(--s1);
  }

  .detail li {
    display: flex;
    justify-content: space-between;
    gap: var(--s4);
    border: none;
  }

  .d-path {
    color: var(--text);
    font-size: var(--fs-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .d-meta {
    color: var(--text-dim);
    font-size: var(--fs-xs);
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .d-err {
    color: var(--err);
    font-size: var(--fs-sm);
    user-select: text;
  }

  .mono {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
  }
</style>
