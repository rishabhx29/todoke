#!/usr/bin/env node
/* gen-icons.mjs — regenerate the PNG icons (favicon-32, apple-touch-icon)
   from the zen-edition mark: cream tile, ink arrival dashes, coral dot.
   Pure Node: SDF shapes → raw RGBA → PNG (zlib + CRC). No deps. */
import fs from "node:fs";
import zlib from "node:zlib";

const CREAM = [0xf2, 0xf0, 0xe3];
const INK = [0x2b, 0x2b, 0x28];
const CORAL = [0xf7, 0x6f, 0x53];

/* signed distance helpers (px space) */
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
const sdCircle = (px, py, cx, cy, r) => Math.hypot(px - cx, py - cy) - r;

function render(size) {
  const s = size / 64; // design grid is 64
  const rows = [];
  for (let y = 0; y < size; y++) {
    const row = Buffer.alloc(size * 4 + 1);
    row[0] = 0; // filter: none
    for (let x = 0; x < size; x++) {
      const px = x + 0.5, py = y + 0.5;
      let rgba = [0, 0, 0, 0];
      const tile = sdRoundRect(px, py, 32 * s, 32 * s, 32 * s, 32 * s, 14 * s);
      if (tile <= 0) {
        rgba = [...CREAM, 255];
        // arrival dashes: full dash, fading dash, then the coral dot
        const d1 = sdCapsule(px, py, 12 * s, 32 * s, 40 * s, 32 * s, 5 * s);
        const d2 = sdCapsule(px, py, 44 * s, 32 * s, 51 * s, 32 * s, 5 * s);
        const dot = sdCircle(px, py, 55.5 * s, 32 * s, 5 * s);
        // dot overlaps the faded dash lane; draw dash, then dot on top
        const dash2 = Math.max(d2, -dot + 0.01); // carve the dot out of dash 2
        if (d1 <= 0) rgba = [...INK, 255];
        else if (dash2 <= 0) rgba = [...INK, 140];
        if (dot <= 0) rgba = [...CORAL, 255];
      }
      const o = 1 + x * 4;
      row[o] = rgba[0]; row[o + 1] = rgba[1]; row[o + 2] = rgba[2]; row[o + 3] = rgba[3];
    }
    rows.push(row);
  }
  return Buffer.concat(rows);
}

function crc32(buf) {
  let c, table = crc32.table;
  if (!table) {
    table = crc32.table = new Int32Array(256);
    for (let n = 0; n < 256; n++) {
      c = n;
      for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
      table[n] = c;
    }
  }
  c = -1;
  for (let i = 0; i < buf.length; i++) c = table[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
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
  ihdr[8] = 8; ihdr[9] = 6; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
  const body = Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", zlib.deflateSync(render(size), { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
  fs.writeFileSync(file, body);
  console.log(`wrote ${file} (${size}x${size}, ${body.length} bytes)`);
}

png(32, "img/favicon-32.png");
png(180, "img/apple-touch-icon.png");
