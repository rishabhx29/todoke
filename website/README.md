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
  img/              favicons, og image
  robots.txt        allow all + sitemap pointer
  sitemap.xml       single-page sitemap
  check.mjs         zero-dependency checker (see below)
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
- **Colors, type, spacing, motion** — design tokens at the top of `css/style.css` (`:root`). One electric accent (`--volt`); everything derives from tokens.
- **Download links** — defined once in `js/main.js` (`DOWNLOAD_BASE` / `LAUNCHED`), consumed by every OS button. Swap the GitHub org/repo in one place on launch day.
- **Meta/social** — `<head>` of `index.html` (title, description, OpenGraph/Twitter, canonical, JSON-LD).

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
