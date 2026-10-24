# dasho.tech

The dasho.tech landing site. Zero-build static: semantic HTML, vanilla CSS, a little vanilla JS. No framework, no bundler, no CDN — point any static host at this directory and it ships.

**Files that move.** Free, open-source, cross-platform file sharing.

## Structure

```
website/
  index.html        the landing page (single page; sections are anchors)
  css/style.css     design system + all styles (tokens at the top)
  js/main.js        progressive enhancement only — page works without it
  fonts/            self-hosted Archivo Variable (latin subset, woff2)
  img/              favicons + og.png social card
  robots.txt        allow all + sitemap pointer
  sitemap.xml       single-page sitemap
  check.mjs         zero-dependency checker (see below)
  tools/snap.mjs    headless-Chrome screenshot probe (viewport + full page)
  .shots/           local screenshots + lighthouse output (gitignored)
```

## Check locally

Requires Node 18+.

```sh
node check.mjs          # serve + validate: HTML well-formedness, links, anchors, assets, CDN-free
node check.mjs --serve  # serve only, for browsing (http://127.0.0.1:4173)
PORT=8080 node check.mjs --serve
```

The checker exits non-zero on errors, so it can gate commits or CI later.

## Edit content

- **Copy** — all text lives in `index.html` as plain semantic HTML. No template, no build: edit, save, deploy.
- **Colors, type, spacing, motion** — design tokens at the top of `css/style.css` (`:root`). One electric accent (`--volt`); everything derives from tokens. One easing language (`--ease-out` + `--dur-*`); new motion should read from those tokens and must respect `prefers-reduced-motion` (the global reduce rule collapses transitions/animations automatically).
- **Meta/social** — `<head>` of `index.html` (title, description, OpenGraph/Twitter, canonical, JSON-LD). The social card image is `img/og.png` (1200x630).

### Launch day: download links

Everything lives in one block at the top of `js/main.js`:

```js
var RELEASES = "https://github.com/dashoapp/dasho/releases";
var LAUNCHED = false;
var ASSETS = { windows: "...", macos: "...", linux: "..." };
```

1. Set `LAUNCHED = true`. Every OS button gets wired automatically and the
   "coming with v1" tags disappear.
2. Optional: fill `ASSETS` with the real release filenames for direct
   `releases/latest/download/<file>` links. Left as placeholders, buttons
   land on the latest-release page instead.
3. Different org/repo? Change `RELEASES` and the few hardcoded GitHub URLs
   in `index.html` (header, hero, trust, footer) — search for `dashoapp`.

Pre-launch, the buttons are placeholder anchors (no href): they render but
nothing links to a 404. The waitlist CTA is a mailto — swap in a form-service
URL at deploy time if you want capture instead of email.

## Verify before shipping

```sh
node check.mjs          # must PASS (pages, links, anchors, assets, CDN-free)
node tools/snap.mjs 360,800 shot.png --full     # 360 px full page, no h-scroll
node tools/snap.mjs 1440,900 shot.png --full --console
node tools/snap.mjs 768,900 shot.png --reduced-motion --full   # motion off
```

Lighthouse mobile run (2026-10-24, local static serve, lighthouse 12.8.2):
performance 98, accessibility 100, best-practices 100, seo 100.
FCP 1.7 s, LCP 2.0 s, TBT 0 ms, CLS 0, SI 2.7 s. Record:
`.shots/lighthouse-summary.json` (regenerated per run):

```sh
node check.mjs --serve &
CHROME_PATH="C:/Program Files/Google/Chrome/Application/chrome.exe" \
  npx lighthouse http://127.0.0.1:4173 --output=json --output-path=.shots/lighthouse.json \
  --chrome-flags="--headless=new --no-sandbox" --quiet
```

## Deploy

No build step. Two known-good options:

**Cloudflare Pages**

1. Create a Pages project → "Direct Upload".
2. Upload the contents of this `website/` directory (not the repo root).
3. Set the custom domain `dasho.tech` in the Pages project settings.

**GitHub Pages**

1. Repo → Settings → Pages → Source: branch `website`, folder `/website`.
2. Custom domain: `dasho.tech`; enforce HTTPS once the DNS check passes.

Any other static server (nginx, `python -m http.server`, Netlify drop) works the same way: serve this directory as the site root.

## Page-weight budget

The whole page stays under ~200 KB (font + CSS + JS + HTML included). When adding anything heavy, lazy-load it and re-run `node check.mjs` plus a Lighthouse mobile pass before shipping.
