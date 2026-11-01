// Shared app state (Svelte 5 runes in a .svelte.ts module).
// Screens read/write this directly; App.svelte owns the global listeners.

import { listen } from "@tauri-apps/api/event";
import {
  getIdentity,
  receiveStatus,
  type Identity,
  type IncomingReceipt,
  type ReceiveStatus,
} from "./api";

export type Screen = "devices" | "send" | "receive" | "settings";

export const app = $state({
  screen: "devices" as Screen,
  identity: null as Identity | null,
  identityError: null as string | null,
  receive: { on: false, port: null, target: null } as ReceiveStatus,
  incoming: [] as IncomingReceipt[],
  /** Destination preselected from the Devices screen. */
  pendingDestination: null as string | null,
});

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
