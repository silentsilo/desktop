import { invoke } from "@tauri-apps/api/core";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateCheckResult =
  | { available: false }
  | { available: true; version: string; body: string | null; update: Update };

export async function checkForUpdate(): Promise<UpdateCheckResult> {
  const update = await check();
  if (!update?.available) {
    return { available: false };
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
