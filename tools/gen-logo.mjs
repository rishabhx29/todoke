#!/usr/bin/env node
/* gen-logo.mjs — render the Todoke mark at master resolution.
 *
 * The mark: a packet in flight. A rounded diamond (the package) moving
 * up-and-right, with two rounded trail dashes decaying behind it along the
 * same diagonal. No letterforms, no lanes — one silhouette that reads at
 * 16 px and still feels designed at 1024.
 *
 *   node tools/gen-logo.mjs            # writes .shots/logo-{a,b,c}.png (1024)
 *
 * Pure Node: SDF shapes -> raw RGBA -> PNG (zlib + CRC), same painter as
 * the website's tools/gen-icons.mjs.
 */
import fs from "node:fs";
import zlib from "node:zlib";

const CORAL = [0xf7, 0x6f, 0x53];
const CREAM = [0xf2, 0xf0, 0xe3];
const INK = [0x2b, 0x2b, 0x28];

/* ---- SDF primitives (px space) ---- */
const sdRoundRect = (px, py, cx, cy, hw, hh, r) => {
  const dx = Math.abs(px - cx) - (hw - r);
  const dy = Math.abs(py - cy) - (hh - r);
  const ax = Math.max(dx, 0), ay = Math.max(dy, 0);
  return Math.hypot(ax, ay) + Math.min(Math.max(dx, dy), 0) - r;
};
const sdRotRoundRect = (px, py, cx, cy, half, r, rad) => {
  const cos = Math.cos(-rad), sin = Math.sin(-rad);
  const dx = px - cx, dy = py - cy;
  return sdRoundRect(dx * cos - dy * sin, dx * sin + dy * cos, 0, 0, half, half, r);
};
const sdCapsule = (px, py, ax, ay, bx, by, r) => {
  const pax = px - ax, pay = py - ay, bax = bx - ax, bay = by - ay;
  const t = Math.max(0, Math.min(1, (pax * bax + pay * bay) / (bax * bax + bay * bay)));
  return Math.hypot(pax - bax * t, pay - bay * t) - r;
};

/* ---- painter: variants provide inside(px,py,s) -> [r,g,b] | null ---- */
function render(size, paint) {
  const s = size / 1024; // design grid
  const rows = [];
  for (let y = 0; y < size; y++) {
    const row = Buffer.alloc(size * 4 + 1);
    row[0] = 0;
    for (let x = 0; x < size; x++) {
      const px = x + 0.5, py = y + 0.5;
      const hit = paint(px, py, s);
      const rgba = hit ?? [0, 0, 0, 0];
      const o = 1 + x * 4;
      row[o] = rgba[0]; row[o + 1] = rgba[1]; row[o + 2] = rgba[2]; row[o + 3] = rgba[3];
    }
    rows.push(row);
  }
  return Buffer.concat(rows);
}

/* ---- PNG encoder ---- */
const crcTable = new Int32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c;
});
function crc32(buf) {
  let c = -1;
  for (let i = 0; i < buf.length; i++) c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ -1) >>> 0;
}
function chunk(type, data) {
  const out = Buffer.alloc(8 + data.length + 4);
  out.writeUInt32BE(data.length, 0);
  out.write(type, 4, "ascii");
  data.copy(out, 8);
  out.writeUInt32BE(crc32(Buffer.concat([Buffer.from(type, "ascii"), data])), 8 + data.length);
  return out;
}
function png(size, pixels, file) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6;
  fs.writeFileSync(file, Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(pixels, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]));
  console.log(`wrote ${file} (${size}x${size})`);
}

/* ---- composites ---- */
// tile(paint, tileColor, r) — clip an inner painter to the rounded tile
function onTile(tileColor, r, paint) {
  return (px, py, s) => {
    if (sdRoundRect(px, py, 512 * s, 512 * s, 512 * s, 512 * s, r * s) > 0) return null;
    return paint(px, py, s) ?? [...tileColor, 255];
  };
}
const firstHit = (...fns) => (px, py, s) => {
  for (const f of fns) {
    const hit = f(px, py, s);
    if (hit) return hit;
  }
  return null;
};

// A/B (packet in flight) — kept from round 1 for comparison
const packetFlight = (packetColor, trailColor) =>
  firstHit(
    (px, py, s) =>
      sdRotRoundRect(px, py, 640 * s, 384 * s, 190 * s, 56 * s, Math.PI / 4) <= 0
        ? [...packetColor, 255]
        : null,
    (px, py, s) => {
      for (const [x1, y1, x2, y2, w] of [[462, 562, 352, 672, 58], [236, 788, 312, 712, 44]]) {
        if (sdCapsule(px, py, x1 * s, y1 * s, x2 * s, y2 * s, (w * s) / 2) <= 0)
          return [...trailColor, 255];
      }
      return null;
    }
  );

// D (arrival lane): one lane line ending mid-tile; the packet rests just past
// its end, floating a touch above — "pulled into the station".
const arrivalLane = (laneColor, packetColor) =>
  firstHit(
    (px, py, s) =>
      sdRotRoundRect(px, py, 724 * s, 560 * s, 148 * s, 44 * s, Math.PI / 4) <= 0
        ? [...packetColor, 255]
        : null,
    (px, py, s) =>
      sdCapsule(px, py, 208 * s, 628 * s, 560 * s, 628 * s, 26 * s) <= 0
        ? [...laneColor, 255]
        : null
  );

// F (the receipt): cream paper, torn zigzag bottom edge, coral check —
// the brand promise ("proof of arrival") as a glyph.
const triangle = (x, period) => {
  const t = (x / period) % 1;
  return t < 0.5 ? t * 2 : 2 - t * 2;
};
const receipt = (paperColor, checkColor, zigColor) => {
  const paper = (px, py, s) => {
    const top = 232 * s, left = 306 * s, right = 718 * s, base = 742 * s;
    if (px < left || px > right || py < top) return null;
    const bottom = base + 38 * s * triangle(px - left, 68 * s); // torn edge
    if (py > bottom) return null;
    // rounded top corners
    const r = 26 * s;
    if (py < top + r && (px < left + r || px > right - r)) {
      const cx = px < left + r ? left + r : right - r;
      const cy = top + r;
      if (Math.hypot(px - cx, py - cy) > r) return null;
    }
    return [...paperColor, 255];
  };
  const check = (px, py, s) =>
    (sdCapsule(px, py, 396 * s, 508 * s, 470 * s, 578 * s, 30 * s) <= 0 ||
      sdCapsule(px, py, 470 * s, 578 * s, 630 * s, 420 * s, 30 * s) <= 0)
      ? [...checkColor, 255]
      : null;
  return firstHit(check, paper);
};

const variants = {
  "logo-a": onTile(CORAL, 220, packetFlight(CREAM, CREAM)),
  "logo-b": onTile(INK, 220, packetFlight(CORAL, CREAM)),
  "logo-d": onTile(INK, 220, arrivalLane(CREAM, CORAL)),
  "logo-e": onTile(INK, 220, receipt(CREAM, CORAL)),
  "logo-f": onTile(CORAL, 220, receipt(CREAM, CORAL)),
};

fs.mkdirSync(".shots", { recursive: true });
for (const [name, spec] of Object.entries(variants)) {
  png(1024, render(1024, spec), `.shots/${name}.png`);
}

// The chosen mark — also write the 1024 master the icon pipeline consumes.
png(1024, render(1024, variants["logo-e"]), "app-icon.png");
