#!/usr/bin/env node
/* test-pair.mjs — does GUI instance A discover GUI instance B over mDNS?
 * Both instances share one WebView2 browser process (same user-data folder),
 * so BOTH page targets live on port 9333 — tell them apart by device name. */
import { spawn, execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const wait = (ms) => new Promise((r) => setTimeout(r, ms));
const GUI = "C:/Rishabh/dasho-gui/target/debug/todoke-gui.exe";
const dataB = path.join(process.env.TEMP ?? "C:/Temp", "todoke-gui-b");
fs.rmSync(dataB, { recursive: true, force: true });
fs.mkdirSync(dataB, { recursive: true });

try { execSync('taskkill /IM todoke-gui.exe /F', { stdio: "ignore", shell: "cmd.exe" }); } catch {}
await wait(700);
// vite first — the exe loads devUrl localhost:1420
if (!(await fetch("http://localhost:1420/").then((r) => r.ok).catch(() => false))) {
  const vite = spawn("npm", ["run", "dev"], { cwd: "C:/Rishabh/dasho-gui/ui", stdio: "ignore", shell: "cmd.exe", detached: true });
  vite.unref();
  for (let i = 0; i < 25; i++) {
    await wait(500);
    if (await fetch("http://localhost:1420/").then((r) => r.ok).catch(() => false)) break;
  }
}
const env = { ...process.env, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: "--remote-debugging-port=9333" };
const a = spawn(GUI, [], { cwd: "C:/Rishabh/dasho-gui", env, stdio: "ignore", detached: true });
a.unref();
const b = spawn(GUI, [], {
  cwd: "C:/Rishabh/dasho-gui",
  env: { ...process.env, TODOKE_DATA_DIR: dataB },
  stdio: "ignore", detached: true,
});
b.unref();

let up = false;
for (let i = 0; i < 60 && !up; i++) {
  await wait(500);
  up = await fetch("http://127.0.0.1:9333/json/version").then((r) => r.ok).catch(() => false);
}
if (!up) { console.error("CDP never came up on 9333"); process.exit(1); }
await wait(3000);

const evalOn = async (targetId, expr) => {
  const list = await (await fetch("http://127.0.0.1:9333/json/list")).json();
  const page = list.find((t) => t.type === "page" && t.id === targetId);
  if (!page) return null;
  const ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener("open", r));
  const out = await new Promise((res) => {
    let id = 0;
    const onMsg = (ev) => {
      const m = JSON.parse(ev.data);
      if (m.id === ++id) { res(m.result?.result?.value); ws.close(); }
    };
    ws.addEventListener("message", onMsg);
    ws.send(JSON.stringify({ id, method: "Runtime.evaluate", params: { expression: expr, returnByValue: true } }));
  });
  return out;
};

// tell the two page targets apart by device name
const pages = (await (await fetch("http://127.0.0.1:9333/json/list")).json()).filter((t) => t.type === "page");
console.log("page targets:", pages.length);
const names = [];
for (const p of pages) {
  const who = await evalOn(p.id, `document.querySelector(".who")?.textContent ?? "?"`);
  names.push({ id: p.id, who });
}
console.log("identities:", JSON.stringify(names));
const aId = names.find((n) => n.who && !n.who.startsWith("todoke-"))?.id ?? names[0]?.id;
const bId = names.find((n) => n.id !== aId)?.id;

// B: turn receive ON so it advertises
if (bId) {
  await evalOn(bId, `document.querySelectorAll(".rail-nav .nav-item")[2].click()`);
  await wait(900);
  await evalOn(bId, `document.querySelector(".switch")?.click()`);
  await wait(1500);
  console.log("B receiving:", await evalOn(bId, `document.body.innerText.includes("Receiving")`));
}

// A: watch the devices list for 30s
let found = 0;
for (let i = 0; i < 10; i++) {
  await wait(3000);
  found = (await evalOn(aId, `document.querySelectorAll(".list .card.row").length`)) ?? 0;
  console.log(`A devices: ${found}`);
  if (found > 0) break;
}
console.log(found > 0 ? "DISCOVERY WORKS" : "DISCOVERY BROKEN");
execSync('taskkill /IM todoke-gui.exe /F', { stdio: "ignore", shell: "cmd.exe" });
process.exit(0);
