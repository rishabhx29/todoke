#!/usr/bin/env node
/* gen-icons.mjs — regenerate the PNG icons (favicon-32, apple-touch-icon)
 * from the Todoke mark: ink tile, cream receipt with a torn zigzag bottom
 * edge, coral check (see dasho-gui/tools/gen-logo.mjs for the master).
 * Pure Node: SDF shapes → raw RGBA → PNG (zlib + CRC). No deps. */
import fs from "node:fs";
import zlib from "node:zlib";

const INK = [0x2b, 0x2b, 0x28];
const CREAM = [0xf2, 0xf0, 0xe3];
const CORAL = [0xf7, 0x6f, 0x53];

/* ---- SDF helpers (design grid: 64) ---- */
const sdRoundRect = (px, py, cx, cy, hw, hh, r) => {
  const dx = Math.abs(px - cx) - (hw - r);
  const dy = Math.abs(py - cy) - (hh - r);
  const ax = Math.max(dx, 0), ay = Math.max(dy, 0);
  return Math.hypot(ax, ay) + Math.min(Math.max(dx, dy), 0) - r;
};
const sdCapsule = (px, py, ax, ay, bx, by, r) => {
  const pax = px - ax, pay = py - ay, bax = bx - ax, bay = by - ay;
  const t = Math.max(0, Math.min(1, (pax * bax + pay * bay) / (bax * bax + bay * bay)));
  return Math.hypot(pax - bax * t, pay - bay * t) - r;
};
const triangle = (x, period) => {
  const t = (x / period) % 1;
  return t < 0.5 ? t * 2 : 2 - t * 2;
};

/* receipt paper: rounded top, torn zigzag bottom — inside test on the 64 grid */
function inPaper(px, py) {
  const left = 19.2, right = 44.8, top = 15, base = 49.3;
  if (px < left || px > right || py < top) return false;
  const bottom = base + 3.9 * triangle(px - left, 12.8 / 3);
  if (py > bottom) return false;
  const r = 4.3;
  if (py < top + r && (px < left + r || px > right - r)) {
    const cx = px < left + r ? left + r : right - r;
    if (Math.hypot(px - cx, py - (top + r)) > r) return false;
  }
  return true;
}

function render(size) {
  const s = size / 64;
  const rows = [];
  for (let y = 0; y < size; y++) {
    const row = Buffer.alloc(size * 4 + 1);
    row[0] = 0;
    for (let x = 0; x < size; x++) {
      const px = (x + 0.5) / s, py = (y + 0.5) / s;
      let rgba = [0, 0, 0, 0];
      if (sdRoundRect(px, py, 32, 32, 32, 32, 14) <= 0) {
        rgba = [...INK, 255];
        if (inPaper(px, py)) rgba = [...CREAM, 255];
        if (
          sdCapsule(px, py, 24, 33, 30.5, 39.5, 2.7) <= 0 ||
          sdCapsule(px, py, 30.5, 39.5, 42.5, 27, 2.7) <= 0
        ) {
          rgba = [...CORAL, 255];
        }
      }
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
function png(size, file) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6;
  fs.writeFileSync(file, Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(render(size), { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]));
  console.log(`wrote ${file} (${size}x${size})`);
}

png(32, "img/favicon-32.png");
png(180, "img/apple-touch-icon.png");
