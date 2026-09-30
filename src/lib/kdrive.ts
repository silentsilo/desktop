/**
 * kDrive (Infomaniak) over WebDAV: the address is built from the drive's
 * numeric ID, which is the number in `kdrive.infomaniak.com/app/drive/<ID>`.
 */

export const KDRIVE_DEFAULT_FOLDER = "SilentSilo";

const KDRIVE_URL = /^https:\/\/(\d+)\.connect\.kdrive\.infomaniak\.com(?:\/(.*))?$/i;

/** Accepts the bare number or a pasted kDrive web address. */
export function kdriveIdFrom(input: string): string {
  const text = input.trim();
  const fromUrl = /\/drive\/(\d+)/.exec(text)?.[1];
  return fromUrl ?? text.replace(/\D/g, "");
}

export function kdriveUrl(id: string, folder: string): string {
  const path = folder
    .split("/")
    .map((part) => part.trim())
    .filter(Boolean)
    .map(encodeURIComponent)
    .join("/");
  return `https://${kdriveIdFrom(id)}.connect.kdrive.infomaniak.com${path ? `/${path}` : ""}`;
}

/** The ID and folder of a saved kDrive address, or null for any other server. */
export function parseKdriveUrl(url: string): { id: string; folder: string } | null {
  const match = KDRIVE_URL.exec(url.trim());
  if (!match) return null;
  const folder = (match[2] ?? "")
    .split("/")
    .filter(Boolean)
    .map((part) => {
      try {
        return decodeURIComponent(part);
      } catch {
        return part;
      }
    })
    .join("/");
  return { id: match[1]!, folder };
}
