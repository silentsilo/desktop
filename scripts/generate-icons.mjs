// Renders public/icon.svg into everything the bundlers and the tray need:
// the PNG set Tauri asks for, a multi-size Windows ICO, a macOS ICNS, and a
// monochrome template for the macOS menu bar. All from one SVG, so the icon
// cannot drift between platforms. Run with `npm run icons`; the output is
// committed, because the release build on the signing machine must not
// depend on a rendering library producing the same bytes twice.

// Imported rather than taken as a global, for the same reason `URL` is in
// check-lockfile.mjs: eslint's config here does not know Node globals.
import { Buffer } from "node:buffer";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { deflateSync } from "node:zlib";

import { Resvg } from "@resvg/resvg-js";
import pngToIco from "png-to-ico";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const svgPath = join(root, "public", "icon.svg");
const svg = readFileSync(svgPath);
const iconsDir = join(root, "src-tauri", "icons");

mkdirSync(iconsDir, { recursive: true });

function renderPng(source, size) {
  const resvg = new Resvg(source, { fitTo: { mode: "width", value: size } });
  return resvg.render().asPng();
}

const targets = [
  ["32x32.png", 32],
  ["128x128.png", 128],
  ["128x128@2x.png", 256],
  ["icon.png", 512],
  ["icon-1024.png", 1024],
];

for (const [name, size] of targets) {
  writeFileSync(join(iconsDir, name), renderPng(svg, size));
}

// Multi-size Windows ICO (16 / 32 / 48 / 256)
const icoSizes = [16, 32, 48, 256];
const ico = await pngToIco(icoSizes.map((size) => renderPng(svg, size)));
writeFileSync(join(iconsDir, "icon.ico"), ico);

// macOS ICNS. The container is a header and a list of typed entries, and
// every type below accepts PNG data as-is, so no other encoder is needed.
// The @2x types are what Retina displays pick; the plain ones are listed
// too so nothing falls back to upscaling.
const icnsTypes = [
  ["icp4", 16],
  ["icp5", 32],
  ["icp6", 64],
  ["ic07", 128],
  ["ic08", 256],
  ["ic09", 512],
  ["ic10", 1024],
  ["ic11", 32],
  ["ic12", 64],
  ["ic13", 256],
  ["ic14", 512],
];

function icnsEntry(type, png) {
  const header = Buffer.alloc(8);
  header.write(type, 0, 4, "ascii");
  header.writeUInt32BE(8 + png.length, 4);
  return Buffer.concat([header, png]);
}

const entries = icnsTypes.map(([type, size]) => icnsEntry(type, renderPng(svg, size)));
const icnsBody = Buffer.concat(entries);
const icnsHeader = Buffer.alloc(8);
icnsHeader.write("icns", 0, 4, "ascii");
icnsHeader.writeUInt32BE(8 + icnsBody.length, 4);
writeFileSync(join(iconsDir, "icon.icns"), Buffer.concat([icnsHeader, icnsBody]));

// CRC table for the PNG chunks below; a `const`, so it has to come first.
const crcTable = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k += 1) {
    c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  }
  return c >>> 0;
});

// The menu bar template: the three squares and the core token, black on
// transparent, no canvas. macOS recolours a template image to match the
// bar, so colour would only be thrown away and the dark canvas would read
// as a black blob. Sized 22pt at 2x. Written as raw RGBA rather than PNG so
// the app can embed it without a PNG decoder.
const glyph = svg
  .toString()
  .replace(/<rect width="512"[^>]*\/>/, "")
  .replace(/stroke="url\(#[a-z-]+\)"/g, 'stroke="#000"')
  .replace(/fill="#10b981"/, 'fill="#000"');
const template = new Resvg(glyph, { fitTo: { mode: "width", value: 44 } }).render();
const rgba = Buffer.from(template.pixels);
for (let i = 0; i < rgba.length; i += 4) {
  rgba[i] = 0;
  rgba[i + 1] = 0;
  rgba[i + 2] = 0;
}
if (template.width !== 44 || template.height !== 44) {
  throw new Error(`template rendered at ${template.width}x${template.height}, expected 44x44`);
}
writeFileSync(join(iconsDir, "tray-template@2x.rgba"), rgba);

// Kept so a reader can see the template; the app embeds the raw file above.
writeFileSync(join(iconsDir, "tray-template@2x.png"), encodePng(rgba, 44, 44));

function encodePng(pixels, width, height) {
  const raw = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y += 1) {
    raw[y * (width * 4 + 1)] = 0;
    pixels.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4);
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr[8] = 8; // bit depth
  ihdr[9] = 6; // RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

function chunk(type, data) {
  const typeBytes = Buffer.from(type, "ascii");
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length, 0);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(Buffer.concat([typeBytes, data])), 0);
  return Buffer.concat([length, typeBytes, data, crc]);
}

function crc32(buf) {
  let c = 0xffffffff;
  for (const byte of buf) {
    c = crcTable[(c ^ byte) & 0xff] ^ (c >>> 8);
  }
  return (c ^ 0xffffffff) >>> 0;
}

console.log(
  `Generated ${targets.length} PNG icons, icon.ico, icon.icns and the tray template in src-tauri/icons from public/icon.svg`,
);
