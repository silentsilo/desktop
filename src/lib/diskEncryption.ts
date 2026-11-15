import { invoke } from "@tauri-apps/api/core";

/** `disk_encryption_status`: `encrypted` is null when the system does not say. */
export type DiskEncryption = { encrypted: boolean | null; os: string };

/** The system maker's page on turning disk encryption on. None for Linux,
 * where it is chosen when the system is installed. */
export const DISK_ENCRYPTION_HELP: Record<string, string | undefined> = {
  windows:
    "https://support.microsoft.com/windows/device-encryption-in-windows-cf7e2b6f-3e70-4882-9532-18633605b7df",
  macos: "https://support.apple.com/guide/mac-help/protect-data-on-your-mac-with-filevault-mh11785/mac",
};

const WARNED_KEY = "silentsilo.diskWarned";

function alreadyWarned(): boolean {
  try {
    return localStorage.getItem(WARNED_KEY) === "1";
  } catch {
    return false;
  }
}

/** True the first time ever a file is opened on a computer whose disk is not
 * encrypted, false after that and whenever the answer is unknown. */
export async function shouldWarnDiskOnce(): Promise<boolean> {
  if (alreadyWarned()) return false;
  try {
    const status = await invoke<DiskEncryption>("disk_encryption_status");
    if (status.encrypted !== false) return false;
    localStorage.setItem(WARNED_KEY, "1");
    return true;
  } catch {
    return false;
  }
}
