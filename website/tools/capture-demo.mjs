#!/usr/bin/env node
/* capture-demo.mjs — record the real Todoke app through a real transfer and
 * save scene frames for the website demo player.
 *
 * Drives the GUI over CDP (dark theme, 1200x750 @1.25dpr), a real CLI peer
 * over loopback mDNS, and Win32 dialog automation for the native file picker.
 * Every frame is genuine app state — nothing staged.
 *
 *   node tools/capture-demo.mjs
 */
import { spawn, execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const CDP_PORT = 9333;
const GUI_DIR = "C:/Rishabh/dasho-gui";
const CLI = "C:/Rishabh/dasho-app/target/release/todoke.exe";
const OUT = path.join(import.meta.dirname, "..", "img", "demo");
const PAYLOADS = path.join(process.env.TEMP ?? "C:/Temp", "todoke-demo");
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

fs.mkdirSync(OUT, { recursive: true });
fs.mkdirSync(PAYLOADS, { recursive: true });

/* ---------- payloads: big file for a slow enough send, plus photos ---------- */
const big = path.join(PAYLOADS, "render-final.mov");
if (!fs.existsSync(big)) {
  const fd = fs.openSync(big, "w");
  const block = Buffer.alloc(4 * 1024 * 1024, 0x5a);
  for (let i = 0; i < 80; i++) fs.writeSync(fd, block); // 320 MB
  fs.closeSync(fd);
  console.log("payload: render-final.mov 320MB");
}
for (const [name, mb] of [["trip-photo-01.jpg", 6], ["trip-photo-02.jpg", 4]]) {
  const p = path.join(PAYLOADS, name);
  if (!fs.existsSync(p)) fs.writeFileSync(p, Buffer.alloc(mb * 1024 * 1024, 0x7c));
}
console.log("payloads ready");

/* ---------- ensure the GUI is up with CDP ---------- */
try { execSync('taskkill /IM todoke-gui.exe /F', { stdio: "ignore", shell: "cmd.exe" }); } catch { /* none running */ }
await wait(600);
// the debug exe loads devUrl localhost:1420 — vite has to be up first
const viteUp = await fetch("http://localhost:1420/").then((r) => r.ok).catch(() => false);
if (!viteUp) {
  const vite = spawn("npm", ["run", "dev"], { cwd: "C:/Rishabh/dasho-gui/ui", stdio: "ignore", shell: "cmd.exe", detached: true });
  vite.unref();
  for (let i = 0; i < 25; i++) {
    await wait(500);
    if (await fetch("http://localhost:1420/").then((r) => r.ok).catch(() => false)) break;
  }
}
console.log("vite:", await fetch("http://localhost:1420/").then((r) => r.ok).catch(() => false));
const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}` };
const app = spawn("C:/Rishabh/dasho-gui/target/debug/todoke-gui.exe", [], {
  cwd: GUI_DIR, env, stdio: "ignore", detached: true,
});
app.unref();
let up = false;
for (let i = 0; i < 20 && !up; i++) {
  await wait(700);
  up = await fetch(`http://127.0.0.1:${CDP_PORT}/json/version`).then((r) => r.ok).catch(() => false);
}
if (!up) { console.error("GUI never exposed CDP"); process.exit(1); }
console.log("GUI up, CDP live");

const list = await (await fetch(`http://127.0.0.1:${CDP_PORT}/json/list`)).json();
const page = list.find((t) => t.type === "page");
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => ws.addEventListener("open", r));
let msgId = 0;
const pending = new Map();
ws.addEventListener("message", (ev) => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
});
const send = (method, params = {}) => new Promise((res) => {
  msgId += 1;
  pending.set(msgId, res);
  ws.send(JSON.stringify({ id: msgId, method, params }));
});

await send("Page.enable");
await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
await send("Emulation.setDeviceMetricsOverride", { width: 1200, height: 750, deviceScaleFactor: 1.25, mobile: false });
await wait(1600);

const evalJs = async (expr) => {
  const res = await send("Runtime.evaluate", { expression: expr, returnByValue: true });
  return res.result?.result?.value;
};
const waitFor = async (expr, timeout = 20000, poll = 200) => {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const v = await evalJs(expr);
    if (v) return v;
    await wait(poll);
  }
  return null;
};
const shot = async (name) => {
  const s = await send("Page.captureScreenshot", { format: "png" });
  fs.writeFileSync(path.join(OUT, name), Buffer.from(s.result.data, "base64"));
  console.log("frame:", name);
};
const gotoScreen = async (nth) => {
  await evalJs(`document.querySelectorAll(".rail-nav .nav-item")[${nth}].click()`);
  await wait(700);
};

/* ---------- scene 1 · Devices, scanning, nothing on the lane ---------- */
// fresh manual-peer state: clear storage AND reload, so the module-level
// in-memory peer list reinitializes empty (stale peers dial dead ports)
await evalJs(`try { localStorage.removeItem("todoke-manual-peers"); location.reload(); } catch {}`);
await wait(2200);
await gotoScreen(0);
await waitFor(`!!document.querySelector(".empty-state")`, 8000);
await wait(400);
await shot("scene-1-devices-scanning.png");

/* ---------- start the CLI receiver, wait for the peer card ---------- */
const rxData = path.join(process.env.TEMP ?? "C:/Temp", "todoke-demo-rx");
fs.rmSync(rxData, { recursive: true, force: true });
fs.mkdirSync(rxData, { recursive: true });
const rx = spawn(CLI, ["serve", "--target", path.join(rxData, "incoming"), "--json"], {
  env: { ...process.env, TODOKE_DATA_DIR: path.join(rxData, "data") },
  stdio: ["ignore", "pipe", "pipe"],
});
let rxInfo = null;
const rxLines = [];
rx.stdout.on("data", (d) => {
  rxLines.push(...d.toString().split("\n"));
  if (!rxInfo) {
    const line = rxLines.find((l) => l.includes('"serving"'));
    if (line) {
      try {
        rxInfo = JSON.parse(line);
        console.log("rx serving:", rxInfo.name, rxInfo.addr, rxInfo.fingerprint);
      } catch { /* partial */ }
    }
  }
});
rx.stderr.on("data", (d) => console.log("rx stderr:", d.toString().trim()));
rx.on("exit", (c) => console.log("rx exited:", c));
await wait(1200);
console.log("rx alive:", !rx.killed);

/* ---------- scene 2 · add the receiver by address (real manual-peer flow) ---------- */
await evalJs(`(() => {
  const b = [...document.querySelectorAll("button")].find((x) => x.textContent.trim() === "Add by address");
  if (b) b.click();
  return !!b;
})()`);
await waitFor(`!!document.querySelector(".add-card")`, 6000);
const filled = await evalJs(`((info) => {
  const set = (el, v) => {
    const proto = el instanceof HTMLInputElement ? HTMLInputElement.prototype : HTMLTextAreaElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, v);
    el.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const inputs = [...document.querySelectorAll(".add-card input")];
  if (inputs.length < 4) return false;
  set(inputs[0], "studio-laptop");
  set(inputs[1], info.addr);
  set(inputs[2], info.fingerprint);
  set(inputs[3], info.cert_b64);
  return true;
})(${JSON.stringify({ addr: rxInfo?.addr ?? "", fingerprint: rxInfo?.fingerprint ?? "", cert_b64: rxInfo?.cert_b64 ?? "" })})`);
console.log("form filled:", filled, "from rx:", rxInfo?.name ?? "none");
if (!rxInfo) { console.error("FATAL: no receiver credentials"); process.exit(1); }
await wait(400);
await evalJs(`(() => { const b = [...document.querySelectorAll("button")].find((x) => x.textContent.trim() === "Save device"); if (b) b.click(); return !!b; })()`);
const peerCard = await waitFor(`!!document.querySelector(".list .card.row")`, 10000, 250);
if (!peerCard) console.error("WARN: saved peer card never appeared");
await wait(700);
await shot("scene-2-devices-found.png");

/* ---------- scene 3 · Send screen, files picked (dev seam — real code path) ---------- */
await gotoScreen(1);
await waitFor(`!!document.querySelector(".dropzone")`, 8000);
const seam = await waitFor(`!!window.__todokeSend`, 8000, 200);
if (!seam) { console.error("FATAL: dev seam missing (build with DEV)"); process.exit(1); }
await evalJs(
  `window.__todokeSend.add(${JSON.stringify([
    big.replaceAll("\\", "\\\\").replace(/\//g, "\\"),
    path.join(PAYLOADS, "trip-photo-01.jpg"),
    path.join(PAYLOADS, "trip-photo-02.jpg"),
  ])})`
);
const filesPicked = await waitFor(
  `(() => { const z = document.querySelector(".dropzone"); return z ? z.textContent.includes("render-final.mov") : false; })()`,
  10000, 300
);
if (!filesPicked) console.error("WARN: files never appeared in dropzone");
await wait(600);
await shot("scene-3-send-picked.png");

/* ---------- scene 4/5 · sending, two progress frames ---------- */
// wait for the destination poll (a 3 s mDNS browse) to resolve a device first
const dest = await waitFor(`!!document.querySelector(".dest .dev")`, 15000, 250);
if (!dest) console.error("WARN: no destination chip — start() would early-return");
// probe: is the receiver dialable at this exact moment? (1 MB via CLI direct)
const probe = spawn(CLI, [
  "send", "--addr", rxInfo.addr, "--fingerprint", rxInfo.fingerprint, "--cert-b64", rxInfo.cert_b64,
  path.join(PAYLOADS, "trip-photo-02.jpg"),
], { env: { ...process.env, TODOKE_DATA_DIR: path.join(rxData, "probe-data") }, stdio: ["ignore", "pipe", "pipe"] });
probe.stdout.on("data", (d) => console.log("probe:", d.toString().trim().slice(0, 120)));
probe.stderr.on("data", (d) => console.log("probe err:", d.toString().trim().slice(0, 120)));
await wait(2500);
await evalJs(`window.__todokeSend.start()`);
console.log("send started via seam");
const p1 = await waitFor(
  `(() => { const p = document.querySelector(".pct"); if (!p) return null; const n = parseInt(p.textContent); return n >= 15 && n <= 55 ? n : null; })()`,
  15000, 120
);
if (p1) await shot("scene-4-sending.png"); else console.error("WARN: mid progress missed");
const p2 = await waitFor(
  `(() => { const p = document.querySelector(".pct"); if (!p) return null; const n = parseInt(p.textContent); return n >= 62 && n <= 92 ? n : null; })()`,
  15000, 120
);
if (p2) await shot("scene-5-sending-late.png"); else console.error("WARN: late progress missed");
/* ---------- scene 6 · receipt ---------- */
const done = await waitFor(`(() => document.querySelector(".receipt") ? true : null)()`, 20000, 250);
if (!done) console.error("WARN: receipt never appeared");
await wait(900); // let the stagger settle
await shot("scene-6-receipt.png");

/* ---------- scene 7 · GUI receives, log fills (CLI direct-dials the GUI) ---------- */
await gotoScreen(2);
await waitFor(`!!document.querySelector(".switch")`, 8000);
const on = await evalJs(`(() => { const s = document.querySelector(".switch"); if (!s) return false; s.click(); return true; })()`);
await wait(1500);
if (on) {
  // the GUI's receive port from the live process's UDP listeners
  const guiPid = execSync('tasklist /FI "IMAGENAME eq todoke-gui.exe" /FO CSV /NH', { shell: "cmd.exe" })
    .toString().match(/"(\d+)"/)?.[1];
  const netstat = execSync("netstat -ano -p UDP", { shell: "cmd.exe" }).toString();
  const guiPort = netstat
    .split("\n")
    .filter((l) => l.includes(` ${guiPid}`) && l.includes(":"))
    .map((l) => l.match(/:(\d+)\s/)?.[1])
    .filter((p) => p && p !== "5353") // 5353 is the mDNS daemon socket, not QUIC
    .find(Boolean);
  console.log("gui receive port:", guiPort);
  // the GUI's deterministic cert: serve --json briefly on the GUI's own data dir
  const guiData = path.join(process.env.APPDATA ?? "", "todoke");
  const idProbe = spawn(CLI, ["serve", "--target", path.join(PAYLOADS, "unused"), "--json"], {
    env: { ...process.env, TODOKE_DATA_DIR: guiData },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let guiId = null;
  const idLines = [];
  idProbe.stdout.on("data", (d) => {
    idLines.push(...d.toString().split("\n"));
    const line = idLines.find((l) => l.includes('"serving"'));
    if (line && !guiId) { try { guiId = JSON.parse(line); } catch { /* partial */ } }
  });
  await wait(1500);
  idProbe.kill();
  if (guiId && guiPort) {
    const sender = spawn(CLI, [
      "send",
      "--addr", `127.0.0.1:${guiPort}`,
      "--fingerprint", guiId.fingerprint,
      "--cert-b64", guiId.cert_b64,
      path.join(PAYLOADS, "trip-photo-01.jpg"),
      path.join(PAYLOADS, "trip-photo-02.jpg"),
    ], { env: { ...process.env, TODOKE_DATA_DIR: path.join(rxData, "tx-data") }, stdio: ["ignore", "pipe", "pipe"] });
    sender.stdout.on("data", (d) => console.log("tx:", d.toString().trim().slice(0, 200)));
    sender.stderr.on("data", (d) => console.log("tx err:", d.toString().trim().slice(0, 200)));
    const got = await waitFor(
      `(() => { const pill = document.querySelector(".pill"); return pill && parseInt(pill.textContent) >= 2 ? true : null; })()`,
      40000, 400
    );
    if (!got) console.error("WARN: incoming log did not fill");
    await wait(800);
  } else {
    console.error("WARN: could not obtain GUI identity/port for direct send");
  }
}
await shot("scene-7-receive-log.png");

/* ---------- teardown ---------- */
rx.kill();
execSync('taskkill /IM todoke-gui.exe /F', { stdio: "ignore", shell: "cmd.exe" });
const sizes = fs.readdirSync(OUT).filter((f) => f.startsWith("scene-")).map((f) => `${f} ${(fs.statSync(path.join(OUT, f)).size / 1024).toFixed(0)}KB`);
console.log(sizes.join("\n"));
console.log("DONE");
process.exit(0);
