import type { CloudKind } from "./types";

/** The provider's own name for its storage. */
export const CLOUD_NAME: Record<CloudKind, string> = {
  onedrive: "OneDrive",
  dropbox: "Dropbox",
  "google-drive": "Google Drive",
};

/** Who sees the folder name: the company, not the product. */
export const CLOUD_COMPANY: Record<CloudKind, string> = {
  onedrive: "Microsoft",
  dropbox: "Dropbox",
  "google-drive": "Google",
};

/** Where the silo folder ends up in the user's account. */
export const CLOUD_PLACE: Record<CloudKind, string> = {
  onedrive: "Apps/SilentSilo on your OneDrive",
  dropbox: "Apps/SilentSilo in your Dropbox",
  "google-drive": "the SilentSilo folder of your Google Drive",
};

/**
 * The folder name a new copy starts with. Neutral on purpose: the provider
 * sees it, and a silo's own name ("Medical", "Divorce") can say too much.
 */
export const DEFAULT_CLOUD_FOLDER = "Silo";

/**
 * The same rule core applies, checked here so the form can say it before
 * anything is sent: one plain segment, none of the characters OneDrive
 * refuses, no dot at either end.
 */
export function cloudFolderProblem(folder: string): string | null {
  const name = folder.trim();
  if (!name) return "Give the folder a name.";
  if (
    name.length > 100 ||
    /["*:<>?/\\|]/.test(name) ||
    // Control characters, which no file system shows.
    // eslint-disable-next-line no-control-regex
    /[\u0000-\u001f\u007f]/.test(name) ||
    name.startsWith(".") ||
    name.endsWith(".")
  ) {
    return 'Use a plain folder name: no slashes, none of " * : < > ? |, no dot at either end.';
  }
  return null;
}
