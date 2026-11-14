<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { setDeviceName } from "../api";
  import { app } from "../state.svelte";
  import { icons } from "../icons";
  import { copyText } from "../clipboard";

  let nameDraft = $state<string | null>(null);
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let savedFlash = $state(false);
  let fpCopied = $state(false);
  let version = $state<string | null>(null);

  getVersion()
    .then((v) => (version = v))
    .catch(() => {});

  const displayName = $derived(nameDraft ?? app.identity?.name ?? "");
  const dirty = $derived(
    app.identity != null && displayName.trim() !== app.identity.name,
  );

  async function saveName() {
    if (saving) return;
    saving = true;
    saveError = null;
    try {
      app.identity = await setDeviceName(displayName);
      nameDraft = null;
      savedFlash = true;
      setTimeout(() => (savedFlash = false), 2000);
    } catch (e) {
      saveError = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  async function copyFp() {
    if (!app.identity) return;
    const ok = await copyText(app.identity.fingerprint);
    if (ok) {
      fpCopied = true;
      setTimeout(() => (fpCopied = false), 1600);
    }
  }
</script>

<header class="page-head">
  <div>
    <h1>Settings</h1>
    <p class="page-sub">This device — who the network sees.</p>
  </div>
</header>

<div class="group">
  <p class="group-label">Device name</p>
  <section class="card">
    <div class="row">
      <input
        type="text"
        value={displayName}
        oninput={(e) => (nameDraft = e.currentTarget.value)}
        maxlength="64"
        aria-label="Device name"
        spellcheck="false"
      />
      <button type="button" class="btn btn-primary" disabled={!dirty || saving} onclick={() => void saveName()}>
        {saving ? "Saving…" : savedFlash ? "Saved ✓" : "Save"}
      </button>
    </div>
    {#if saveError}
      <p class="row-note error" role="alert">{saveError}</p>
    {:else}
      <p class="row-note">Renaming while receive mode is on restarts the announcement.</p>
    {/if}
  </section>
</div>

<div class="group">
  <p class="group-label">Fingerprint</p>
  <section class="card">
    <div class="row">
      <div class="fp-copy">
        <code class="fp mono">{app.identity?.fingerprint ?? "—"}</code>
        <p class="row-note fp-note">
          The short code friends can check out-of-band to be sure it's really you.
        </p>
      </div>
      <button type="button" class="btn btn-ghost" onclick={() => void copyFp()}>
        <span class="ico ico-16">{@html icons.copy}</span>
        {fpCopied ? "Copied ✓" : "Copy"}
      </button>
    </div>
  </section>
</div>

<div class="group">
  <p class="group-label">Folders</p>
  <section class="card">
    <dl class="paths">
      <div class="row">
        <dt>Data directory</dt>
        <dd>{app.identity?.dataDir ?? "—"}</dd>
      </div>
      <div class="row">
        <dt>Staging (in-progress transfers)</dt>
        <dd>{app.identity?.stagingDir ?? "—"}</dd>
      </div>
      <div class="row">
        <dt>Default receive target</dt>
        <dd>{app.identity?.defaultTarget ?? "—"}</dd>
      </div>
    </dl>
  </section>
</div>

<footer class="foot">
  <p>
    Todoke {version ? `v${version}` : ""} — free and open-source (MIT OR
    Apache-2.0). No accounts. No telemetry. Transfers stay on your network.
  </p>
</footer>

<style>
  .group {
    max-width: 46rem;
    margin-bottom: 24px;
  }

  .group-label {
    font-size: var(--fs-xs);
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-2);
    margin: 0 0 6px 12px;
  }

  .card {
    padding: 4px 16px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    min-height: 44px;
    padding: 8px 0;
  }

  .row + .row,
  .row-note {
    border-top: 1px solid var(--sep);
  }

  .row-note {
    color: var(--text-2);
    font-size: var(--fs-sm);
    margin: 0;
    padding: 8px 0;
    min-height: 32px;
  }

  .error {
    color: var(--err);
    user-select: text;
  }

  input {
    flex: 1;
    min-width: 0;
    background: var(--inset);
    border: 1px solid transparent;
    border-radius: var(--r-input);
    padding: 7px 11px;
    font-size: var(--fs-body);
    transition:
      border-color var(--dur-1) var(--ease-out),
      box-shadow var(--dur-1) var(--ease-out);
  }

  input:focus {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 3px var(--accent-soft);
  }

  .fp-copy {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
    flex: 1;
  }

  .fp {
    font-size: 11.5px;
    color: var(--text);
    background: var(--inset);
    border-radius: var(--r-input);
    padding: 7px 11px;
    letter-spacing: 0.03em;
    user-select: text;
    overflow-x: auto;
    white-space: nowrap;
  }

  .fp-note {
    border-top: none;
    padding: 0;
    min-height: 0;
  }

  .btn-ghost {
    align-self: center;
  }

  .paths {
    margin: 0;
  }

  .paths .row {
    justify-content: flex-start;
  }

  .paths dt {
    color: var(--text);
    font-size: var(--fs-body);
    flex: none;
  }

  .paths dd {
    margin: 0;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-family: var(--mono);
    text-align: right;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    user-select: text;
    min-width: 0;
    flex: 1;
  }

  .foot {
    color: var(--text-3);
    font-size: var(--fs-xs);
    border-top: 1px solid var(--sep);
    padding-top: 16px;
    max-width: 46rem;
  }
</style>
