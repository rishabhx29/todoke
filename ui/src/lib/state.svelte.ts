// Shared app state (Svelte 5 runes in a .svelte.ts module).
// Screens read/write this directly; App.svelte owns the global listeners.

import { listen } from "@tauri-apps/api/event";
import {
  getIdentity,
  receiveStatus,
  type Device,
  type Identity,
  type IncomingReceipt,
  type ReceiveStatus,
} from "./api";

export type Screen = "devices" | "send" | "receive" | "settings";

/** A peer the user added by hand — networks that block mDNS (campus/public
 *  Wi-Fi with client isolation) still work this way. Persisted locally. */
export type ManualPeer = { name: string; addr: string; fingerprint: string; certB64: string };
const MANUAL_KEY = "todoke-manual-peers";
function loadManualPeers(): ManualPeer[] {
  try { return JSON.parse(localStorage.getItem(MANUAL_KEY) ?? "[]") as ManualPeer[]; }
  catch { return []; }
}

export const app = $state({
  screen: "devices" as Screen,
  identity: null as Identity | null,
  identityError: null as string | null,
  receive: { on: false, port: null, target: null } as ReceiveStatus,
  incoming: [] as IncomingReceipt[],
  /** Destination preselected from the Devices screen. */
  pendingDestination: null as string | null,
  manualPeers: loadManualPeers() as ManualPeer[],
});

export function addManualPeer(p: ManualPeer) {
  app.manualPeers = [...app.manualPeers.filter((x) => x.fingerprint !== p.fingerprint), p];
  try { localStorage.setItem(MANUAL_KEY, JSON.stringify(app.manualPeers)); } catch { /* private mode */ }
}

export function removeManualPeer(fingerprint: string) {
  app.manualPeers = app.manualPeers.filter((x) => x.fingerprint !== fingerprint);
  try { localStorage.setItem(MANUAL_KEY, JSON.stringify(app.manualPeers)); } catch { /* private mode */ }
}

/** Discovered devices plus saved manual peers (mDNS presence wins). */
export function withManualPeers(found: Device[]): Device[] {
  const seen = new Set(found.map((d) => d.fingerprint));
  const manual = app.manualPeers
    .filter((p) => !seen.has(p.fingerprint))
    .map((p) => {
      // discovered records carry IP-only addr + separate port; api.ts
      // recomposes "addr:port" on send — store the same shape
      const [host, portStr] = p.addr.split(":");
      return {
        name: p.name,
        fingerprint: p.fingerprint,
        certB64: p.certB64,
        addr: host,
        port: Number(portStr ?? 0) || 0,
        manual: true,
      };
    });
  return [...found, ...manual];
}

export function go(screen: Screen) {
  app.screen = screen;
}

export async function refreshIdentity(): Promise<void> {
  try {
    app.identity = await getIdentity();
    app.identityError = null;
  } catch (e) {
    app.identityError = String(e);
  }
}

export async function refreshReceive(): Promise<void> {
  try {
    app.receive = await receiveStatus();
  } catch {
    /* status is best-effort */
  }
}

/** Global event wiring — call once from App.svelte's $effect. */
export function wireGlobalEvents(): () => void {
  const unlisten = listen<IncomingReceipt>("receive://incoming", (e) => {
    app.incoming = [e.payload, ...app.incoming].slice(0, 200);
  });
  return () => {
    unlisten.then((fn) => fn());
  };
}
