import { invoke } from "@tauri-apps/api/core";
import type { BrowserExtensionStatus } from "./types";

export async function readBrowserExtension(): Promise<BrowserExtensionStatus> {
  return invoke<BrowserExtensionStatus>("browser_extension_status");
}

/** Saves the setting and opens or closes the pipe; answers the new state. */
export async function writeBrowserExtension(enabled: boolean): Promise<BrowserExtensionStatus> {
  return invoke<BrowserExtensionStatus>("browser_extension_set", { enabled });
}
