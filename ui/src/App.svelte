<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Devices from "./lib/screens/Devices.svelte";
  import Send from "./lib/screens/Send.svelte";
  import Receive from "./lib/screens/Receive.svelte";
  import Settings from "./lib/screens/Settings.svelte";
  import { icons } from "./lib/icons";
  import { app, go, refreshIdentity, refreshReceive, wireGlobalEvents } from "./lib/state.svelte";

  const win = getCurrentWindow();

  const nav: { id: typeof app.screen; label: string; icon: string }[] = [
    { id: "devices", label: "Devices", icon: icons.laptop },
    { id: "send", label: "Send", icon: icons.send },
    { id: "receive", label: "Receive", icon: icons.receive },
    { id: "settings", label: "Settings", icon: icons.gear },
  ];

  $effect(() => {
    void refreshIdentity();
    void refreshReceive();
    return wireGlobalEvents();
  });

  function onRailKeydown(e: KeyboardEvent) {
    const idx = nav.findIndex((n) => n.id === app.screen);
    if (e.key === "ArrowDown" || e.key === "ArrowRight") {
      e.preventDefault();
      go(nav[(idx + 1) % nav.length].id);
    } else if (e.key === "ArrowUp" || e.key === "ArrowLeft") {
      e.preventDefault();
      go(nav[(idx - 1 + nav.length) % nav.length].id);
    }
  }
</script>

<div class="shell">
  <!-- Frameless-window titlebar: traffic lights + full-width drag region -->
  <div class="titlebar" data-tauri-drag-region>
    <div class="lights" data-tauri-drag-region>
      <button
        type="button"
        class="tl tl-close"
        aria-label="Close window"
        onclick={() => void win.close()}
      >
        <svg viewBox="0 0 10 10" aria-hidden="true">
          <path d="M3 3l4 4M7 3 3 7" />
        </svg>
      </button>
      <button
        type="button"
        class="tl tl-min"
        aria-label="Minimize window"
        onclick={() => void win.minimize()}
      >
        <svg viewBox="0 0 10 10" aria-hidden="true">
          <path d="M2.6 5h4.8" />
        </svg>
      </button>
      <button
        type="button"
        class="tl tl-zoom"
        aria-label="Zoom window"
        onclick={() => void win.toggleMaximize()}
      >
        <svg viewBox="0 0 10 10" aria-hidden="true">
          <path d="M2.4 7.6V5.1l2.5 2.5z" />
          <path d="M7.6 2.4H5.1l2.5 2.5z" />
        </svg>
      </button>
    </div>
  </div>

  <aside class="rail">
    <div class="brand">
      <span class="wordmark">todoke</span>
      <span class="tagline">it arrives</span>
    </div>
    <nav class="rail-nav" aria-label="Screens">
      {#each nav as item (item.id)}
        <button
          type="button"
          class="nav-item"
          class:active={app.screen === item.id}
          aria-current={app.screen === item.id ? "page" : undefined}
          onclick={() => go(item.id)}
          onkeydown={onRailKeydown}
        >
          <span class="ico ico-16">{@html item.icon}</span>
          {item.label}
          {#if item.id === "receive" && app.receive.on}
            <span class="dot" title="Receive is on" aria-label="Receive is on"></span>
          {/if}
        </button>
      {/each}
    </nav>
    <div class="rail-foot">
      <p class="who">{app.identity?.name ?? "…"}</p>
      <p class="foot-note">No accounts. No telemetry.</p>
    </div>
  </aside>

  <main class="content">
    {#if app.screen === "devices"}
      <Devices />
    {:else if app.screen === "send"}
      <Send />
    {:else if app.screen === "receive"}
      <Receive />
    {:else}
      <Settings />
    {/if}
  </main>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: var(--rail-w) 1fr;
    grid-template-rows: var(--titlebar-h) 1fr;
    height: 100dvh;
  }

  /* ---- Titlebar -------------------------------------------------------- */

  .titlebar {
    grid-column: 1 / -1;
    grid-row: 1;
    position: relative;
    z-index: 10;
    display: flex;
    align-items: center;
    padding-left: 12px;
  }

  .lights {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .tl {
    width: 10px;
    height: 10px;
    padding: 0;
    border-radius: 50%;
    display: grid;
    place-items: center;
    transition:
      transform var(--dur-1) var(--ease-out),
      filter var(--dur-1) var(--ease-out);
  }

  .tl svg {
    width: 10px;
    height: 10px;
    stroke: rgba(0, 0, 0, 0.55);
    stroke-width: 1.3;
    stroke-linecap: round;
    fill: none;
    opacity: 0;
    transition: opacity var(--dur-1) var(--ease-out);
  }

  .tl-zoom svg path {
    fill: rgba(0, 0, 0, 0.55);
    stroke: none;
  }

  .lights:hover .tl svg {
    opacity: 1;
  }

  .tl-close {
    background: #ff5f57;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
  }

  .tl-min {
    background: #febc2e;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
  }

  .tl-zoom {
    background: #28c840;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.12);
  }

  .tl:disabled {
    filter: saturate(0);
    opacity: 0.85;
  }

  /* ---- Sidebar ---------------------------------------------------------- */

  .rail {
    grid-column: 1;
    grid-row: 1 / -1;
    display: flex;
    flex-direction: column;
    padding: calc(var(--titlebar-h) + 6px) 10px 14px;
    background: var(--sidebar);
    backdrop-filter: blur(20px) saturate(180%);
    -webkit-backdrop-filter: blur(20px) saturate(180%);
    border-right: 1px solid var(--sep);
  }

  .brand {
    display: flex;
    flex-direction: column;
    gap: 1px;
    padding: 0 10px 18px;
  }

  .wordmark {
    color: var(--text);
    font-size: var(--fs-section);
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .tagline {
    color: var(--text-2);
    font-size: var(--fs-xs);
  }

  .rail-nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: 9px;
    height: 32px;
    padding: 0 10px;
    border-radius: var(--r-btn);
    color: var(--text-2);
    font-size: var(--fs-body);
    font-weight: 500;
    text-align: left;
    transition:
      color var(--dur-1) var(--ease-out),
      background var(--dur-1) var(--ease-out),
      transform var(--dur-1) var(--ease-out);
  }

  .nav-item:hover {
    color: var(--text);
    background: var(--fill);
  }

  .nav-item.active {
    color: var(--accent);
    background: var(--accent-soft);
    font-weight: 600;
  }

  .dot {
    width: 6px;
    height: 6px;
    margin-left: auto;
    border-radius: 50%;
    background: var(--ok);
    position: relative;
  }

  .dot::after {
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

  .rail-foot {
    margin-top: auto;
    padding: 0 10px;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .who {
    color: var(--text);
    font-size: var(--fs-sm);
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .foot-note {
    color: var(--text-3);
    font-size: var(--fs-xs);
  }

  /* ---- Content ---------------------------------------------------------- */

  .content {
    grid-column: 2;
    grid-row: 2;
    min-width: 0;
    overflow-y: auto;
    padding: 20px 24px 32px;
  }
</style>
