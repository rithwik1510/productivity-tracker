// Regenerates every app and tray icon from the SVGs in src-tauri/icons/source: `bun run icons`.
// logo.svg is the master (48-unit grid). logo-{16,20,24,32}.svg and tray-{16,20,24,32}.svg are
// the same mark redrawn on each size's pixel grid, so small icons stay crisp instead of being a
// blurred downscale. Rasterized headlessly in Edge, like the e2e, so nothing opens on screen.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { chromium } from "playwright";

const ICONS = "src-tauri/icons";
const SOURCE = `${ICONS}/source`;
/** The small-icon sizes at 100, 125, 150 and 200 % scale, each with its own tuned SVG. */
const TUNED = [16, 20, 24, 32];
const ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 256];
/** Tray glyph colours, named for the file: white for a dark taskbar, black for a light one. */
const TRAY = { white: "#ffffff", black: "#1a1a1a" };

const svg = (name) => readFileSync(`${SOURCE}/${name}.svg`, "utf8");
const logoSvg = (size) => svg(TUNED.includes(size) ? `logo-${size}` : "logo");

const browser = await chromium.launch({ channel: "msedge" });
const page = await browser.newPage();

/** Draws `source` at size×size: the PNG, and the straight-alpha RGBA pixels. */
async function raster(source, size) {
  const sized = source.replace("<svg ", `<svg width="${size}" height="${size}" `);
  const { png, rgba } = await page.evaluate(
    async ([sized, size]) => {
      const base64 = (bytes) => {
        let s = "";
        for (const b of bytes) s += String.fromCharCode(b);
        return btoa(s);
      };
      const img = new Image();
      img.src = `data:image/svg+xml,${encodeURIComponent(sized)}`;
      await img.decode();
      const canvas = new OffscreenCanvas(size, size);
      const ctx = canvas.getContext("2d");
      ctx.drawImage(img, 0, 0);
      const blob = await canvas.convertToBlob({ type: "image/png" });
      return {
        png: base64(new Uint8Array(await blob.arrayBuffer())),
        rgba: base64(ctx.getImageData(0, 0, size, size).data),
      };
    },
    [sized, size],
  );
  return { png: Buffer.from(png, "base64"), rgba: Buffer.from(rgba, "base64") };
}

/** A 32-bit BMP icon frame: BGRA rows bottom-up, then the 1-bit mask (set where transparent). */
function bmpFrame(size, rgba) {
  const maskRow = Math.ceil(size / 32) * 4;
  const pixels = size * size * 4;
  const out = Buffer.alloc(40 + pixels + maskRow * size);
  out.writeUInt32LE(40, 0);
  out.writeInt32LE(size, 4);
  out.writeInt32LE(size * 2, 8); // colour rows plus mask rows
  out.writeUInt16LE(1, 12);
  out.writeUInt16LE(32, 14);
  out.writeUInt32LE(pixels + maskRow * size, 20);
  for (let y = 0; y < size; y++) {
    const row = size - 1 - y;
    for (let x = 0; x < size; x++) {
      const src = (y * size + x) * 4;
      const dst = 40 + (row * size + x) * 4;
      out[dst] = rgba[src + 2];
      out[dst + 1] = rgba[src + 1];
      out[dst + 2] = rgba[src];
      out[dst + 3] = rgba[src + 3];
      if (rgba[src + 3] === 0) out[40 + pixels + row * maskRow + (x >> 3)] |= 0x80 >> (x & 7);
    }
  }
  return out;
}

/** BMP frames below 256 and a PNG frame at 256, the layout Windows' own tools write. */
function ico(frames) {
  const header = Buffer.alloc(6 + 16 * frames.length);
  header.writeUInt16LE(1, 2);
  header.writeUInt16LE(frames.length, 4);
  let offset = header.length;
  const images = frames.map(({ size, png, rgba }, i) => {
    const data = size >= 256 ? png : bmpFrame(size, rgba);
    const at = 6 + 16 * i;
    header[at] = size >= 256 ? 0 : size;
    header[at + 1] = size >= 256 ? 0 : size;
    header.writeUInt16LE(1, at + 4);
    header.writeUInt16LE(32, at + 6);
    header.writeUInt32LE(data.length, at + 8);
    header.writeUInt32LE(offset, at + 12);
    offset += data.length;
    return data;
  });
  return Buffer.concat([header, ...images]);
}

// The large PNGs and icon.icns come from the master via Tauri's own generator. It also writes
// Android and iOS sets, which this Windows-only app doesn't ship.
execFileSync("bun", ["run", "tauri", "icon", `${SOURCE}/logo.svg`, "-o", ICONS], {
  stdio: "inherit",
});
rmSync(`${ICONS}/android`, { recursive: true, force: true });
rmSync(`${ICONS}/ios`, { recursive: true, force: true });

// Replace Tauri's downscaled ICO and 32 px PNG with ones whose small frames are the tuned SVGs.
const frames = [];
for (const size of ICO_SIZES) frames.push({ size, ...(await raster(logoSvg(size), size)) });
writeFileSync(`${ICONS}/icon.ico`, ico(frames));
writeFileSync(`${ICONS}/32x32.png`, frames.find((f) => f.size === 32).png);

mkdirSync(`${ICONS}/tray`, { recursive: true });
for (const size of TUNED) {
  for (const [name, colour] of Object.entries(TRAY)) {
    const { png } = await raster(svg(`tray-${size}`).replaceAll("#ffffff", colour), size);
    writeFileSync(`${ICONS}/tray/${name}-${size}.png`, png);
  }
}

// The README's copy of the mark.
copyFileSync(`${SOURCE}/logo.svg`, "docs/logo.svg");

await browser.close();
