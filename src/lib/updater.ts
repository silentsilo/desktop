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

/** Downloads and installs in one step, then relaunches into the new version. */
export async function installUpdateAndRelaunch(
  update: Update,
  onProgress?: (downloaded: number, contentLength: number | null) => void,
): Promise<void> {
  let downloaded = 0;
  let contentLength: number | null = null;

  await update.downloadAndInstall((event) => {
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

  await relaunch();
}
