import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { isNewerVersion } from "./updateSchedule";

/**
 * Updates are installed by the user, never by the app: it says a newer
 * version exists and opens the download page (docs/ARCHITECTURE.md,
 * "Updates", for why that is a security decision). Fixed here, not taken
 * from the answer, so a forged answer cannot send anyone elsewhere.
 */
export const DOWNLOAD_PAGE = "https://silentsilo.com/download/";

export type UpdateCheckResult = { available: false } | { available: true; version: string };

/** Asks which version is the latest (the website, or its repository on
 * GitHub when the site does not answer) and compares it with this one. */
export async function checkForUpdate(): Promise<UpdateCheckResult> {
  const latest = await invoke<string>("update_latest_version");
  return isNewerVersion(latest, __APP_VERSION__)
    ? { available: true, version: latest }
    : { available: false };
}

export function openDownloadPage(open: (url: string) => Promise<void> = openUrl): Promise<void> {
  return open(DOWNLOAD_PAGE);
}
