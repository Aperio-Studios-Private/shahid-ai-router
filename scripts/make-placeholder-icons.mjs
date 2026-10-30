// Generates placeholder application icons (solid brand blue with a white
// router glyph block) without external dependencies. Run:
//   node scripts/make-placeholder-icons.mjs
// Outputs to apps/desktop/src-tauri/icons/.
import { deflateSync } from "node:zlib";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const outDir = join(root, "apps", "desktop", "src-tauri", "icons");
mkdirSync(outDir, { recursive: true });

const crcTable = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const typeBuf = Buffer.from(type, "ascii");
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])));
  return Buffer.concat([len, typeBuf, data, crc]);
}

/** Renders a 1024x1024 RGBA canvas: brand blue field, white "S" block. */
function renderCanvas(size) {
  const px = Buffer.alloc(size * size * 3);
  const R = 0x00, G = 0x6e, B = 0xb9; // brand blue (#006eb9)
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const u = x / size, v = y / size;
      // white rounded-rect band with a gap, suggesting routing arrows
      const inBand = v > 0.42 && v < 0.58 && u > 0.18 && u < 0.82;
      const inDot = Math.hypot(u - 0.5, v - 0.75) < 0.06;
      const color = inBand || inDot ? [255, 255, 255] : [R, G, B];
      const i = (y * size + x) * 3;
      px[i] = color[0]; px[i + 1] = color[1]; px[i + 2] = color[2];
    }
  }
  return px;
}

function encodePng(size, px) {
  const signature = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8;  // bit depth
  ihdr[9] = 2;  // color type RGB
  const stride = size * 3;
  const raw = Buffer.alloc((stride + 1) * size);
  for (let y = 0; y < size; y++) {
    raw[y * (stride + 1)] = 0; // filter none
    px.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  return Buffer.concat([
    signature,
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

/** Nearest-neighbor downscale. */
function resize(px, from, to) {
  const out = Buffer.alloc(to * to * 3);
  for (let y = 0; y < to; y++) {
    for (let x = 0; x < to; x++) {
      const sx = Math.min(from - 1, Math.floor((x / to) * from));
      const sy = Math.min(from - 1, Math.floor((y / to) * from));
      const si = (sy * from + sx) * 3;
      const di = (y * to + x) * 3;
      out[di] = px[si]; out[di + 1] = px[si + 1]; out[di + 2] = px[si + 2];
    }
  }
  return out;
}

/** ICO wrapping a PNG entry (Vista+ supports PNG-compressed icons). */
function encodeIco(png) {
  const dir = Buffer.alloc(6);
  dir.writeUInt16LE(0, 0);
  dir.writeUInt16LE(1, 2); // type: icon
  dir.writeUInt16LE(1, 4); // count
  const entry = Buffer.alloc(16);
  entry[0] = 0; entry[1] = 0; // 0 = 256+ px
  entry[2] = 0; entry[3] = 0;
  entry.writeUInt16LE(1, 4);  // planes
  entry.writeUInt16LE(32, 6); // bpp
  entry.writeUInt32LE(png.length, 8);
  entry.writeUInt32LE(22, 12); // data offset
  return Buffer.concat([dir, entry, png]);
}

const base = renderCanvas(1024);
const png1024 = encodePng(1024, base);
writeFileSync(join(outDir, "app-icon.png"), png1024);
writeFileSync(join(outDir, "icon.ico"), encodeIco(png1024));
writeFileSync(join(outDir, "32x32.png"), encodePng(32, resize(base, 1024, 32)));
writeFileSync(join(outDir, "128x128.png"), encodePng(128, resize(base, 1024, 128)));
writeFileSync(join(outDir, "128x128@2x.png"), encodePng(256, resize(base, 1024, 256)));
console.log("icons written to", outDir);
