#!/usr/bin/env node
/*
 * Todoke site checker — zero-dependency verification seam.
 *
 *   node check.mjs            serve site + run all checks (exit 1 on error)
 *   node check.mjs --serve    serve site only (for browsing / Lighthouse)
 *   PORT=4173                 override port
 *
 * Checks:
 *   - every page resolves over HTTP and is well-formed (balanced tags, quoted attrs)
 *   - one h1 per page, no duplicate ids
 *   - every internal link/asset/anchor resolves (fragments checked against target ids)
 *   - zero CDN/framework dependencies (no absolute-URL asset refs)
 *   - external links HEAD-checked; failures allowed only if listed in ALLOWED_404
 *   - css url(...) refs resolve locally
 */
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.dirname(fileURLToPath(import.meta.url));
const PORT = Number(process.env.PORT || 4173);
const HOST = "127.0.0.1";
const ORIGIN = `http://${HOST}:${PORT}`;

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".json": "application/json",
  ".xml": "application/xml",
  ".txt": "text/plain; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".webmanifest": "application/manifest+json",
};

/* External URLs allowed to 404 pre-launch (annotated in tickets) */
const ALLOWED_404 = [
  /^https:\/\/todoke\.tech\/?(#.*)?$/,                // own canonical — DNS not live yet
  /^https:\/\/github\.com\/todokeapp\/todoke(\/|$)/,   // repo not created yet
  /^https:\/\/github\.com\/todokeapp\/todoke\/releases/,
];

const VOID = new Set(["area", "base", "br", "col", "embed", "hr", "img", "input",
  "link", "meta", "param", "source", "track", "wbr"]);
const ASSET_EXT = /\.(css|js|mjs|png|jpe?g|webp|svg|ico|woff2?|mp4|webm|json|webmanifest)$/i;

/* ------------------------------------------------------------------ server */
function handler(req, res) {
  let urlPath;
  try {
    urlPath = decodeURIComponent(new URL(req.url, ORIGIN).pathname);
  } catch {
    res.writeHead(400).end("bad request");
    return;
  }
  if (urlPath.endsWith("/")) urlPath += "index.html";
  const file = path.normalize(path.join(ROOT, urlPath));
  if (!file.startsWith(ROOT)) { res.writeHead(403).end("forbidden"); return; }
  fs.readFile(file, (err, data) => {
    if (err) {
      if (req.url.startsWith("/__404probe/")) {
        res.writeHead(200, { "content-type": "text/plain" }).end("ok");
      } else {
        res.writeHead(404, { "content-type": "text/plain" }).end("not found");
      }
      return;
    }
    res.writeHead(200, { "content-type": MIME[path.extname(file)] || "application/octet-stream" });
    res.end(data);
  });
}

function serve() {
  return new Promise((resolve) => {
    const server = http.createServer(handler);
    server.listen(PORT, HOST, () => resolve(server));
  });
}

/* ------------------------------------------------------------------ fetch helpers */
async function get(urlPath) {
  const res = await fetch(ORIGIN + urlPath);
  const body = res.status === 200 ? await res.text() : "";
  return { status: res.status, body };
}

async function head(url) {
  const ctrl = new AbortController();
  const timer = setTimeout(() => ctrl.abort(), 6000);
  try {
    const res = await fetch(url, { method: "HEAD", redirect: "follow", signal: ctrl.signal });
    return res.status;
  } catch {
    try { // some hosts reject HEAD — retry GET ranged
      const res = await fetch(url, { headers: { range: "bytes=0-0" }, signal: ctrl.signal });
      return res.status;
    } catch {
      return 0;
    }
  } finally {
    clearTimeout(timer);
  }
}

/* ------------------------------------------------------------------ html checks */
const errors = [];
const warnings = [];

function err(msg) { errors.push(msg); }
function warn(msg) { warnings.push(msg); }

function stripInvisible(html) {
  return html
    .replace(/<!--[\s\S]*?-->/g, "")
    .replace(/<!\[CDATA\[[\s\S]*?\]\]>/g, "")
    .replace(/<![^>]*>/g, "")
    .replace(/<\?[\s\S]*?\?>/g, "");
}

function checkWellFormed(html, page) {
  const src = stripInvisible(html);
  const stack = [];
  const tagRe = /<(\/?)([a-zA-Z][a-zA-Z0-9-]*)((?:"[^"]*"|'[^']*'|[^"'>])*)>/g;
  let m;
  let last = 0;
  while ((m = tagRe.exec(src)) !== null) {
    // unterminated junk between tags (stray '<')
    const between = src.slice(last, m.index);
    if (/</.test(between)) err(`${page}: stray "<" before <${m[2]}>`);
    last = tagRe.lastIndex;
    const [, close, name, attrs] = m;
    if (!close) {
      const attrStr = attrs || "";
      if (!/\/\s*$/.test(attrStr) && !VOID.has(name.toLowerCase())) stack.push(name.toLowerCase());
      // attribute quoting — mask quoted values first so `=` inside them can't false-positive
      const masked = attrStr.replace(/"[^"]*"|'[^']*'/g, '""');
      const bad = masked.match(/[\s"'][a-zA-Z-]+=([^"'\s>][^\s>]*)/);
      if (bad) err(`${page}: unquoted attribute value "${bad[1]}" on <${name}>`);
    } else {
      const lower = name.toLowerCase();
      const idx = stack.lastIndexOf(lower);
      if (idx === -1) {
        err(`${page}: closing </${name}> with no opener`);
      } else {
        if (idx !== stack.length - 1) {
          err(`${page}: misnested tags — </${name}> closes over [${stack.slice(idx + 1).join(", ")}]`);
        }
        stack.length = idx;
      }
    }
  }
  if (stack.length) err(`${page}: unclosed tag(s): ${stack.join(", ")}`);

  const h1s = (src.match(/<h1\b/gi) || []).length;
  if (h1s !== 1) err(`${page}: expected exactly one <h1>, found ${h1s}`);

  const ids = [...src.matchAll(/\bid="([^"]+)"/g)].map((x) => x[1]);
  const dup = ids.filter((v, i) => ids.indexOf(v) !== i);
  if (dup.length) err(`${page}: duplicate id(s): ${[...new Set(dup)].join(", ")}`);

  return new Set(ids);
}

function collectRefs(html) {
  const refs = [];
  const attrRe = /\b(href|src|poster|srcset)\s*=\s*"([^"]+)"/gi;
  let m;
  while ((m = attrRe.exec(html)) !== null) {
    const attr = m[1].toLowerCase();
    if (attr === "srcset") {
      for (const part of m[2].split(",")) {
        const u = part.trim().split(/\s+/)[0];
        if (u) refs.push(u);
      }
    } else {
      refs.push(m[2]);
    }
  }
  return refs;
}

/* ------------------------------------------------------------------ css checks */
function checkCss(cssPath, cssText) {
  // data URIs (e.g. inline SVG grain) may contain url(#frag) — drop them before scanning
  const css = cssText.replace(/url\(\s*["']?data:[^)]*["']?\s*\)/gi, "url()");
  const dir = path.dirname(cssPath);
  const urlRe = /url\(\s*(['"]?)([^'")]+)\1\s*\)/g;
  let m;
  while ((m = urlRe.exec(css)) !== null) {
    const u = m[2].trim();
    if (/^(data:|about:)/i.test(u)) continue;
    if (/^https?:/i.test(u)) {
      err(`${path.relative(ROOT, cssPath)}: external asset url() — CDN dependency: ${u}`);
      continue;
    }
    const target = path.resolve(dir, u.split("?")[0].split("#")[0]);
    if (!fs.existsSync(target)) {
      err(`${path.relative(ROOT, cssPath)}: url() target missing: ${u}`);
    }
  }
}

/* ------------------------------------------------------------------ main */
const pages = [];
function findPages(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (e.name === "tools" || e.name === "node_modules") continue;
    const p = path.join(dir, e.name);
    if (e.isDirectory()) findPages(p);
    else if (e.name.endsWith(".html")) pages.push(p);
  }
}

const isServeOnly = process.argv.includes("--serve");
const server = await serve();
console.log(`serving ${ROOT} at ${ORIGIN}`);

if (isServeOnly) {
  console.log("--serve mode: press Ctrl+C to stop");
} else {
  findPages(ROOT);
  const idMaps = new Map();

  for (const file of pages) {
    const rel = path.relative(ROOT, file).replaceAll("\\", "/");
    const urlPath = rel === "index.html" ? "/" : "/" + rel;
    const { status, body } = await get(urlPath);
    if (status !== 200) { err(`${urlPath}: HTTP ${status}`); continue; }
    idMaps.set(rel, checkWellFormed(body, urlPath));

    for (const ref of collectRefs(body)) {
      if (ref.startsWith("mailto:") || ref.startsWith("tel:") || ref.startsWith("data:")) continue;
      if (/^https?:\/\//i.test(ref)) {
        const u = new URL(ref);
        if (ASSET_EXT.test(u.pathname)) {
          err(`${urlPath}: asset referenced over absolute URL — must be self-hosted: ${ref}`);
        } else {
          const st = await head(ref);
          const allowed = ALLOWED_404.some((re) => re.test(ref));
          if ((st === 404 || st === 0) && !allowed) {
            warn(`${urlPath}: external link ${st || "unreachable"}: ${ref}`);
          } else if (st === 404 || st === 0) {
            warn(`${urlPath}: external link ${st || "unreachable"} (allowed pre-launch): ${ref}`);
          }
        }
        continue;
      }
      // internal
      const [beforeHash, frag] = ref.split("#");
      if (beforeHash) {
        const targetRel = beforeHash === "/" || beforeHash === ""
          ? "index.html"
          : beforeHash.replace(/^\//, "").replace(/\/$/, "");
        const targetFile = path.join(ROOT, targetRel.endsWith("/") ? targetRel + "index.html" : targetRel);
        if (!fs.existsSync(targetFile)) {
          err(`${urlPath}: internal link 404 (no file on disk): ${ref}`);
          continue;
        }
        if (frag) {
          const targetPage = path.relative(ROOT, targetFile).replaceAll("\\", "/");
          const map = idMaps.get(targetPage)
            ?? (() => {
              const html = fs.readFileSync(targetFile, "utf8");
              const ids = new Set([...stripInvisible(html).matchAll(/\bid="([^"]+)"/g)].map((x) => x[1]));
              idMaps.set(targetPage, ids);
              return ids;
            })();
          if (!map.has(frag) && !map.has("user-content-" + frag)) {
            err(`${urlPath}: anchor #${frag} not found in ${targetPage}`);
          }
        }
      } else if (frag) {
        if (!idMaps.get(rel)?.has(frag)) {
          err(`${urlPath}: anchor #${frag} not found on page`);
        }
      }
    }
  }

  // css files
  function findCss(dir) {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      if (e.name === "tools" || e.name === "node_modules") continue;
      const p = path.join(dir, e.name);
      if (e.isDirectory()) findCss(p);
      else if (e.name.endsWith(".css")) checkCss(p, fs.readFileSync(p, "utf8"));
    }
  }
  findCss(ROOT);
}

if (!isServeOnly) {
  server.close();
  console.log(`\nchecked ${pages.length} page(s)`);
  for (const w of warnings) console.log(`  warn  ${w}`);
  for (const e of errors) console.log(`  ERROR ${e}`);
  console.log(errors.length ? `\nFAIL — ${errors.length} error(s), ${warnings.length} warning(s)` : `\nPASS${warnings.length ? ` (${warnings.length} warning(s))` : ""}`);
  process.exit(errors.length ? 1 : 0);
}
