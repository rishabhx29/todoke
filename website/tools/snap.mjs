#!/usr/bin/env node
/*
 * snap.mjs — headless Chrome probe for the Todoke site (zero deps, Node 22+).
 *
 *   node tools/snap.mjs 360,740 out.png [--reduced-motion] [--console] [--full]
 *
 * Starts the site server + a headless Chrome with CDP, emulates the given
 * viewport (real device metrics — no desktop min-window artifact), optionally
 * emulates prefers-reduced-motion, captures console errors, and screenshots.
 * Exits 1 if console errors were captured (with --console) .
 */
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const CHROME = process.env.CHROME
  || "C:/Program Files/Google/Chrome/Application/chrome.exe";
const SITE_PORT = 4190;
const CDP_PORT = 9223;

const args = process.argv.slice(2);
const sizeArg = args.find((a) => /^\d+,\d+$/.test(a)) || "1440,900";
const out = args.find((a) => !a.startsWith("-") && a !== sizeArg) || "shot.png";
const [width, height] = sizeArg.split(",").map(Number);
const reduced = args.includes("--reduced-motion");
const wantConsole = args.includes("--console");
const fullPage = args.includes("--full");
const atIdx = args.indexOf("--at");
const themeIdx = args.indexOf("--theme");
const theme = themeIdx !== -1 ? args[themeIdx + 1] : null;
const at = atIdx !== -1 ? args[atIdx + 1] : null; // CSS selector or Y px

const MIME = { ".html": "text/html; charset=utf-8", ".css": "text/css",
  ".js": "text/javascript", ".mjs": "text/javascript", ".svg": "image/svg+xml",
  ".png": "image/png", ".woff2": "font/woff2", ".xml": "application/xml",
  ".txt": "text/plain" };

const site = http.createServer((req, res) => {
  let p;
  try { p = decodeURIComponent(new URL(req.url, "http://x").pathname); }
  catch { res.writeHead(400).end(); return; }
  if (p.endsWith("/")) p += "index.html";
  const file = path.normalize(path.join(ROOT, p));
  if (!file.startsWith(ROOT)) { res.writeHead(403).end(); return; }
  fs.readFile(file, (err, data) => {
    if (err) { res.writeHead(404).end(); return; }
    res.writeHead(200, { "content-type": MIME[path.extname(file)] || "application/octet-stream" });
    res.end(data);
  });
});
await new Promise((r) => site.listen(SITE_PORT, "127.0.0.1", r));

const chrome = spawn(CHROME, [
  "--headless=new", "--disable-gpu", "--no-sandbox", "--no-first-run",
  `--remote-debugging-port=${CDP_PORT}`,
  "--user-data-dir=" + path.join(os.tmpdir(), "todoke-snap-profile"),
  "about:blank",
], { stdio: "ignore" });

async function getTarget() {
  for (let i = 0; i < 40; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${CDP_PORT}/json/list`);
      const list = await res.json();
      const page = list.find((t) => t.type === "page");
      if (page) return page.webSocketDebuggerUrl;
    } catch {}
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error("chrome CDP did not come up");
}

const wsUrl = await getTarget();
const ws = new WebSocket(wsUrl);
await new Promise((r) => ws.onopen = r);

let msgId = 0;
const pending = new Map();
const consoleErrors = [];
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); }
  else if (msg.method === "Log.entryAdded" && msg.params.entry.level === "error")
    consoleErrors.push(msg.params.entry.text);
  else if (msg.method === "Runtime.exceptionThrown")
    consoleErrors.push(msg.params.exceptionDetails.text
      + (msg.params.exceptionDetails.exception?.description || ""));
};
function send(method, params = {}) {
  return new Promise((resolve) => {
    const id = ++msgId;
    pending.set(id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });
}

await send("Page.enable");
await send("Log.enable");
await send("Network.enable");
await send("Network.setCacheDisabled", { cacheDisabled: true });
await send("Runtime.enable");
await send("Emulation.setDeviceMetricsOverride", {
  width, height, deviceScaleFactor: 2, mobile: width < 700,
});
if (reduced) {
  await send("Emulation.setEmulatedMedia", {
    features: [{ name: "prefers-reduced-motion", value: "reduce" }],
  });
}
await send("Runtime.enable");
await send("Page.navigate", { url: `http://127.0.0.1:${SITE_PORT}/` });
await new Promise((r) => setTimeout(r, 1200));
if (theme) {
  // set on the real origin, then reload so the head script reads it
  await send("Runtime.evaluate", { expression: `localStorage.setItem("todoke-theme", "${theme}")` });
  await send("Page.reload");
  await new Promise((r) => setTimeout(r, 1500));
}

if (at) {
  // scroll to a selector (e.g. "#how") or a Y px value, let reveals settle
  await send("Runtime.evaluate", {
    expression: `(async () => {
      window.__todokeLenisDestroy?.();
      document.documentElement.style.scrollBehavior = 'auto';
      const raw = ${JSON.stringify(at)};
      let targetY = Number(raw);
      if (!Number.isFinite(targetY)) {
        try {
          const el = document.querySelector(raw);
          if (el) targetY = el.getBoundingClientRect().top + scrollY;
        } catch (e) { /* bad selector: stay put */ }
      }
      scrollTo(0, targetY);
      await new Promise((r) => setTimeout(r, 400));
    })()`,
    awaitPromise: true,
  }).then((r) => console.log("AT-DEBUG:", JSON.stringify(r.result ?? r)));
  // long enough for batched reveals (1.4 s tween + stagger) to finish
  await new Promise((r) => setTimeout(r, 3400));
}

if (fullPage) {
  // scroll through the page so IntersectionObserver reveals fire, then return to top
  await send("Runtime.evaluate", {
    expression: `(async () => {
      document.documentElement.style.scrollBehavior = 'auto';
      const h = innerHeight;
      for (let y = 0; y <= document.documentElement.scrollHeight; y += h) {
        scrollTo(0, y);
        await new Promise(r => setTimeout(r, 140));
      }
      scrollTo(0, 0);
    })()`,
    awaitPromise: true,
  });
  await new Promise((r) => setTimeout(r, 900));
}

const shot = await send("Page.captureScreenshot", {
  format: "png",
  ...(fullPage ? { captureBeyondViewport: true } : {}),
});
fs.writeFileSync(out, Buffer.from(shot.result.data, "base64"));

const { result: { result: { value: layout } } } = await send("Runtime.evaluate", {
  expression: `JSON.stringify({
    scrollW: document.documentElement.scrollWidth,
    clientW: document.documentElement.clientWidth,
    hScroll: document.documentElement.scrollWidth > document.documentElement.clientWidth,
    scrollY: Math.round(scrollY)
  })`,
  returnByValue: true,
});
console.log(`shot ${width}x${height}${reduced ? " reduced-motion" : ""} -> ${out}`);
console.log(`layout: ${layout}`);

await ws.close();
chrome.kill();
site.close();
if (wantConsole) {
  if (consoleErrors.length) {
    console.log("CONSOLE ERRORS:");
    for (const e of consoleErrors) console.log("  " + e);
    process.exit(1);
  }
  console.log("console: clean (no errors)");
}
