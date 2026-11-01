// Typed bridge to the Tauri commands (see src-tauri/src/commands.rs).
// Naming: command args are camelCase on this side (Tauri maps to the Rust
// snake_case params); payload fields are camelCase via serde.

import { invoke, Channel } from "@tauri-apps/api/core";

export interface Identity {
  name: string;
  fingerprint: string;
  dataDir: string;
  stagingDir: string;
  defaultTarget: string;
}

export interface Device {
  name: string;
  fingerprint: string;
  certB64: string;
  addr: string;
  port: number;
}

export interface ReceiptFile {
  path: string;
  size: number;
  hash: string;
  chunks: number;
}

export interface Receipt {
  manifestHash: string;
  files: ReceiptFile[];
}

export type SendEvent =
  | { event: "fileStarted"; data: { path: string; size: number } }
  | { event: "chunkSent"; data: { path: string; chunk: number; bytes: number } }
  | { event: "fileSent"; data: { path: string } }
  | { event: "finished"; data: { receipt: Receipt } }
  | { event: "failed"; data: { message: string } };

export interface IncomingReceipt {
  fromAddr: string;
  manifestHash: string;
  files: ReceiptFile[];
  totalBytes: number;
  ok: boolean;
  error: string | null;
  atUnixMs: number;
}

export interface ReceiveStatus {
  on: boolean;
  port: number | null;
  target: string | null;
}

export interface PathsStats {
  files: number;
  totalBytes: number;
  skipped: number;
}

export function getIdentity(): Promise<Identity> {
  return invoke("get_identity");
}

export function setDeviceName(name: string): Promise<Identity> {
  return invoke("set_device_name", { name });
}

export function listDevices(timeoutMs = 3000): Promise<Device[]> {
  return invoke("list_devices", { timeoutMs });
}

/** Starts a send session; resolves with the session id immediately. */
export function sendFiles(
  paths: string[],
  device: Device,
  onEvent: Channel<SendEvent>,
): Promise<number> {
  return invoke("send_files", {
    paths,
    addr: `${device.addr}:${device.port}`,
    fingerprint: device.fingerprint,
    certB64: device.certB64,
    onEvent,
  });
}

export function cancelTransfer(session: number): Promise<void> {
  return invoke("cancel_transfer", { session });
}

export function receiveStart(target?: string): Promise<ReceiveStatus> {
  return invoke("receive_start", { target: target ?? null });
}

export function receiveStop(): Promise<ReceiveStatus> {
  return invoke("receive_stop");
}

export function receiveStatus(): Promise<ReceiveStatus> {
  return invoke("receive_status");
}

export function getReceiveTarget(): Promise<string> {
  return invoke("get_receive_target");
}

export function setReceiveTarget(target: string): Promise<ReceiveStatus> {
  return invoke("set_receive_target", { target });
}

export function statPaths(paths: string[]): Promise<PathsStats> {
  return invoke("stat_paths", { paths });
}
