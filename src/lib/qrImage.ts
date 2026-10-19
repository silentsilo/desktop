import { invoke } from "@tauri-apps/api/core";

/// Larger pictures are scaled down first: a 4K screenshot still leaves a
/// two-factor QR code several pixels a module, and the pixels cross IPC raw.
const MAX_SIDE = 2400;

/// The image a paste carries, such as a screenshot from Win+Shift+S, or
/// `null` when it is text.
export function pastedImage(data: DataTransfer | null): File | null {
  for (const item of Array.from(data?.items ?? [])) {
    if (item.kind === "file" && item.type.startsWith("image/")) return item.getAsFile();
  }
  return null;
}

/// The text of the QR code in a picture, or `null` when there is none. The
/// webview decodes the picture, so any format it shows works; Rust finds and
/// reads the code (`commands/qr.rs`). Throws when the picture will not open.
export async function qrFromImage(picture: Blob): Promise<string | null> {
  const bitmap = await createImageBitmap(picture);
  const scale = Math.min(1, MAX_SIDE / Math.max(bitmap.width, bitmap.height));
  const width = Math.max(1, Math.round(bitmap.width * scale));
  const height = Math.max(1, Math.round(bitmap.height * scale));
  const canvas = document.createElement("canvas");
  canvas.width = width;
  canvas.height = height;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) throw new Error("canvas");
  // White under a transparent PNG, so its clear parts read as paper.
  ctx.fillStyle = "#fff";
  ctx.fillRect(0, 0, width, height);
  ctx.drawImage(bitmap, 0, 0, width, height);
  bitmap.close();
  const { data } = ctx.getImageData(0, 0, width, height);
  const grey = new Uint8Array(width * height);
  for (let i = 0; i < grey.length; i++) {
    grey[i] = (data[i * 4]! * 299 + data[i * 4 + 1]! * 587 + data[i * 4 + 2]! * 114) / 1000;
  }
  return invoke<string | null>("qr_from_pixels", grey, {
    headers: { "x-width": String(width), "x-height": String(height) },
  });
}
