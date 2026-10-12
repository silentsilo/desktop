import { invoke } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateCheckResult =
  | { available: false }
  | { available: true; version: string; body: string | null; update: Update };

/** Where SilentSilo's releases are published, one folder per tag. */
const RELEASES = "https://github.com/silentsilo/desktop/releases/download/";

/** Said when an answer points anywhere else. */
const NOT_OURS =
  "This update does not come from SilentSilo's releases, so it was not offered.\u001ferr.update_not_ours";

/** Every download address an update answer names, in its platform map or
 * on its own. */
export function updateUrls(raw: Record<string, unknown>): string[] {
  const urls: string[] = [];
  if (typeof raw.url === "string") urls.push(raw.url);
  const platforms = raw.platforms;
  if (platforms && typeof platforms === "object") {
    for (const entry of Object.values(platforms as Record<string, unknown>)) {
      const url = (entry as { url?: unknown } | null)?.url;
      if (typeof url === "string") urls.push(url);
    }
  }
  return urls;
}

/**
 * Whether every file an update offers sits in SilentSilo's releases, under
 * the tag of the version it claims. The signature says who built a file,
 * not which version it is, so a server answering for the updater could
 * offer an older signed installer as a newer one. Only the repository's
 * owners can put a file under a tag, and a tag names its version.
 */
export function fromOurReleases(version: string, raw: Record<string, unknown>): boolean {
  const folder = `${RELEASES}v${version}/`;
  const urls = updateUrls(raw);
  return (
    urls.length > 0 &&
    urls.every((url) => {
      let name: string;
      try {
        const href = new URL(url).href;
        if (!href.startsWith(folder)) return false;
        name = decodeURIComponent(href.slice(folder.length));
      } catch {
        return false;
      }
      return name.length > 0 && !/[/\\]/.test(name) && !name.includes("..");
    })
  );
}

export async function checkForUpdate(): Promise<UpdateCheckResult> {
  // Never back to an older version, whatever the server says.
  const update = await check({ allowDowngrades: false });
  if (!update?.available) {
    return { available: false };
  }
  if (!fromOurReleases(update.version, update.rawJson)) {
    throw new Error(NOT_OURS);
  }
  return { available: true, version: update.version, body: update.body ?? null, update };
}

/** A failed install, and whether it failed after the silos were locked, in
 * which case the screen that started it is no longer there to say so. */
export class UpdateInstallError extends Error {
  constructor(
    readonly reason: unknown,
    readonly silosLocked: boolean,
  ) {
    super(String(reason));
  }
}

/**
 * Downloads, locks every open silo, installs, then relaunches into the new
 * version. On Windows the installer ends this process outright, with no exit
 * event, so a silo left open would leave its decrypted working copy on disk.
 */
export async function installUpdateAndRelaunch(
  update: Update,
  onProgress?: (downloaded: number, contentLength: number | null) => void,
): Promise<void> {
  let downloaded = 0;
  let contentLength: number | null = null;

  try {
    await update.download((event) => {
      switch (event.event) {
        case "Started":
          contentLength = event.data.contentLength ?? null;
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          onProgress?.(downloaded, contentLength);
          break;
        case "Finished":
          break;
      }
    });
  } catch (e) {
    throw new UpdateInstallError(e, false);
  }

  // A lock that failed part way may still have closed some of them.
  try {
    await invoke("vault_lock", { id: null });
    await update.install();
    await relaunch();
  } catch (e) {
    throw new UpdateInstallError(e, true);
  }
}
