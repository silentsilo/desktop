import { invoke } from "@tauri-apps/api/core";

export type AutostartStatus = {
  /** False where the OS integration does not exist yet (anything but Windows). */
  supported: boolean;
  enabled: boolean;
};

/** Asks the OS, not a stored preference: turning the entry off in Task
 * Manager's Startup tab has to show up here too. */
export async function readAutostart(): Promise<AutostartStatus> {
  return invoke<AutostartStatus>("autostart_status");
}

export async function writeAutostart(enabled: boolean): Promise<void> {
  await invoke("autostart_set", { enabled });
}
