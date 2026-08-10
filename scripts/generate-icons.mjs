import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { Resvg } from "@resvg/resvg-js";
import pngToIco from "png-to-ico";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const svgPath = join(root, "public", "icon.svg");
const svg = readFileSync(svgPath);
const iconsDir = join(root, "src-tauri", "icons");

mkdirSync(iconsDir, { recursive: true });

const targets = [
  ["32x32.png", 32],
  ["128x128.png", 128],
  ["128x128@2x.png", 256],
  ["icon.png", 512],
  ["icon-1024.png", 1024],
];

const pngBuffers = [];

for (const [name, size] of targets) {
  const resvg = new Resvg(svg, {
    fitTo: { mode: "width", value: size },
  });
  const buf = resvg.render().asPng();
  writeFileSync(join(iconsDir, name), buf);
  pngBuffers.push({ name, size, buf });
}

// Multi-size Windows ICO (16 / 32 / 48 / 256)
const icoSizes = [16, 32, 48, 256];
const icoPngs = icoSizes.map((size) => {
  const resvg = new Resvg(svg, {
    fitTo: { mode: "width", value: size },
  });
  return resvg.render().asPng();
});

const ico = await pngToIco(icoPngs);
writeFileSync(join(iconsDir, "icon.ico"), ico);

console.log(
  `Generated ${targets.length} PNG icons + icon.ico in src-tauri/icons from public/icon.svg`,
);
