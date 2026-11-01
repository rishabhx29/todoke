<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { setDeviceName } from "../api";
  import { app } from "../state.svelte";
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

<header class="head">
  <div>
    <h1>Settings</h1>
    <p class="sub">This device — who the network sees.</p>
  </div>
</header>

<section class="panel">
  <h2>Device name</h2>
  <div class="name-row">
    <input
      type="text"
      value={displayName}
      oninput={(e) => (nameDraft = e.currentTarget.value)}
      maxlength="64"
      aria-label="Device name"
      spellcheck="false"
    />
    <button type="button" class="cta" disabled={!dirty || saving} onclick={() => void saveName()}>
      {saving ? "Saving…" : savedFlash ? "Saved ✓" : "Save"}
    </button>
  </div>
  {#if saveError}
    <p class="error" role="alert">{saveError}</p>
  {:else}
    <p class="note">Renaming while receive mode is on restarts the announcement.</p>
  {/if}
</section>

<section class="panel">
  <h2>Fingerprint</h2>
  <p class="note">
    The short code friends can check out-of-band to be sure it's really you.
  </p>
  <div class="fp-row">
    <code class="fp">{app.identity?.fingerprint ?? "—"}</code>
    <button type="button" class="ghost" onclick={() => void copyFp()}>
      {fpCopied ? "Copied ✓" : "Copy"}
    </button>
  </div>
</section>

<section class="panel">
  <h2>Folders</h2>
  <dl class="paths">
    <div>
      <dt>Data directory</dt>
      <dd>{app.identity?.dataDir ?? "—"}</dd>
    </div>
    <div>
      <dt>Staging (in-progress transfers)</dt>
      <dd>{app.identity?.stagingDir ?? "—"}</dd>
    </div>
    <div>
      <dt>Default receive target</dt>
      <dd>{app.identity?.defaultTarget ?? "—"}</dd>
    </div>
  </dl>
</section>

<footer class="foot">
  <p>
    Todoke {version ? `v${version}` : ""} — free and open-source (MIT OR
    Apache-2.0). No accounts. No telemetry. Transfers stay on your network.
  </p>
</footer>

<style>
  .head {
    margin-bottom: var(--s5);
  }

  h1 {
    font-size: var(--fs-h1);
  }

  h2 {
    font-size: var(--fs-h3);
    margin-bottom: var(--s2);
  }

  .sub {
    color: var(--text-dim);
    font-size: var(--fs-sm);
    margin-top: var(--s1);
  }

  .panel {
    border: 1px dashed var(--line);
    border-radius: var(--r-tile);
    padding: var(--s5);
    max-width: 46rem;
    margin-bottom: var(--s4);
  }

  .name-row {
    display: flex;
    gap: var(--s3);
  }

  input {
    flex: 1;
    background: var(--ink-1);
    border: 1px solid var(--line);
    border-radius: var(--r-tile);
    padding: var(--s2) var(--s3);
  }

  input:focus {
    border-color: var(--volt);
    outline: none;
  }

  .cta {
    background: var(--volt);
    color: var(--volt-ink);
    font-weight: 640;
    padding: var(--s2) var(--s4);
    border-radius: var(--r-pill);
  }

  .cta:disabled {
    opacity: 0.4;
    cursor: default;
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

  .ghost:hover {
    color: var(--volt);
    border-color: var(--volt);
  }

  .note {
    color: var(--text-dim);
    font-size: var(--fs-sm);
    margin-top: var(--s3);
  }

  .error {
    color: var(--err);
    font-size: var(--fs-sm);
    margin-top: var(--s3);
    user-select: text;
  }

  .fp-row {
    display: flex;
    align-items: center;
    gap: var(--s3);
    flex-wrap: wrap;
  }

  .fp {
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 1.05rem;
    color: var(--text-hi);
    background: var(--ink-1);
    border: 1px solid var(--line);
    border-radius: var(--r-tile);
    padding: var(--s2) var(--s3);
    letter-spacing: 0.04em;
    user-select: text;
  }

  .paths {
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--s3);
  }

  .paths dt {
    color: var(--text-dim);
    font-size: var(--fs-xs);
    text-transform: uppercase;
    letter-spacing: 0.08em;
  }

  .paths dd {
    margin: 2px 0 0;
    color: var(--text);
    font-size: var(--fs-sm);
    user-select: text;
    word-break: break-all;
  }

  .foot {
    color: var(--text-dim);
    font-size: var(--fs-xs);
    border-top: 1px dashed var(--line);
    padding-top: var(--s4);
    max-width: 46rem;
  }
</style>
