# todoke.tech

The todoke.tech landing site. Zero-build static: semantic HTML, vanilla CSS, a little vanilla JS. No framework, no bundler, no CDN — point any static host at this directory and it ships.

**Files that arrive.** Free, open-source, cross-platform file sharing.

## Structure

```
website/
  index.html        the landing page (single page; sections are anchors)
  css/style.css     design system + all styles (tokens at the top)
  js/main.js        choreography: Lenis + GSAP scrubs + anime.js counters (page works without it)
  js/hero3d.js      Three.js packet-stream hero (lazy: loads only when visible + motion OK)
  fonts/            self-hosted woff2: Bricolage Grotesque (text), Instrument Serif
                    + italic (display), JetBrains Mono (labels) — latin subsets
  img/              favicons + og.png social card (regen PNGs: node tools/gen-icons.mjs)
  vendor/           gsap 3.12 (+ ScrollTrigger, CustomEase), lenis 1.1 (ESM),
                    anime.js 3.2, three 0.170 (lazy) — all local, no CDN
  robots.txt        allow all + sitemap pointer
  sitemap.xml       single-page sitemap
  check.mjs         zero-dependency checker (see below)
  tools/snap.mjs    headless-Chrome screenshot probe (viewport + full page, --dsf, scrollbars hidden)
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
- **Colors, type, spacing, motion** — design tokens at the top of `css/style.css` (`:root`): cream paper `--paper`, ink `--ink`, one coral accent family (`--coral` fills / `--coral-text` small text / `--coral-big` large serif), night band tokens for the download + footer. Display type is Instrument Serif (italic = the accent voice); text is Bricolage Grotesque; labels/terminal are JetBrains Mono. One easing language (`--ease`); new motion should read from those tokens and must respect `prefers-reduced-motion` (the global reduce rule collapses transitions/animations automatically).
- **Meta/social** — `<head>` of `index.html` (title, description, OpenGraph/Twitter, canonical, JSON-LD). The social card image is `img/og.png` (1200x630; regenerate with `node tools/snap.mjs 1200,630 img/og.png --dsf 1`).

### Launch day: download links

The three OS tiles in the download band live in `index.html` (search `dl-grid`).
They all point at the GitHub releases page and carry a "coming with v1" tag:

1. On launch, replace each tile's `href` with the real asset URL
   (`https://github.com/todokeapp/todoke/releases/latest/download/<file>`).
2. Delete the `<span class="dl__note">` lines.
3. Different org/repo? Search `index.html` for `todokeapp` and swap the URLs.

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
3. Set the custom domain `todoke.tech` in the Pages project settings.

**GitHub Pages**

1. Repo → Settings → Pages → Source: branch `website`, folder `/website`.
2. Custom domain: `todoke.tech`; enforce HTTPS once the DNS check passes.

Any other static server (nginx, `python -m http.server`, Netlify drop) works the same way: serve this directory as the site root.

## Page-weight budget

First load is ~200 KB over the wire (fonts are 151 KB of it; HTML/CSS/JS gzip
down hard; three.js ~690 KB is lazy and only downloads when the hero is on
screen, WebGL exists, and motion is allowed). When adding anything heavy,
lazy-load it and re-run `node check.mjs` plus a Lighthouse mobile pass before
shipping.
