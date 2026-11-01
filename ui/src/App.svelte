<script lang="ts">
  import Devices from "./lib/screens/Devices.svelte";
  import Send from "./lib/screens/Send.svelte";
  import Receive from "./lib/screens/Receive.svelte";
  import Settings from "./lib/screens/Settings.svelte";
  import { app, go, refreshIdentity, refreshReceive, wireGlobalEvents } from "./lib/state.svelte";

  const nav: { id: typeof app.screen; label: string }[] = [
    { id: "devices", label: "Devices" },
    { id: "send", label: "Send" },
    { id: "receive", label: "Receive" },
    { id: "settings", label: "Settings" },
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

  <div class="lane" aria-hidden="true"></div>

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
    display: flex;
    height: 100dvh;
  }

  .rail {
    width: var(--rail-w);
    flex: none;
    display: flex;
    flex-direction: column;
    padding: var(--s5) var(--s4);
    background: var(--ink-0);
  }

  .brand {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 0 var(--s2) var(--s5);
  }

  .wordmark {
    color: var(--text-hi);
    font-size: 1.45rem;
    font-weight: 720;
    letter-spacing: -0.02em;
  }

  .tagline {
    color: var(--volt);
    font-size: var(--fs-xs);
    font-weight: 560;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .rail-nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .nav-item {
    display: flex;
    align-items: center;
    gap: var(--s2);
    text-align: left;
    padding: var(--s2) var(--s3);
    border-radius: var(--r-tile);
    color: var(--text-dim);
    font-weight: 540;
    transition:
      color var(--dur-1) var(--ease-out),
      background var(--dur-1) var(--ease-out);
  }

  .nav-item:hover {
    color: var(--text-hi);
    background: var(--ink-2);
  }

  .nav-item.active {
    color: var(--volt);
    background: var(--ink-1);
    font-weight: 640;
  }

  .dot {
    width: 7px;
    height: 7px;
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

  .rail-foot {
    margin-top: auto;
    padding: 0 var(--s3);
    display: flex;
    flex-direction: column;
    gap: 2px;
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
    color: var(--text-dim);
    font-size: var(--fs-xs);
  }

  .lane {
    width: 1px;
    flex: none;
    background: var(--lane-dash);
    background-size: 2.5rem 100%;
    margin: var(--s4) 0;
  }

  .content {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: var(--s6) var(--s6) var(--s7);
  }
</style>
