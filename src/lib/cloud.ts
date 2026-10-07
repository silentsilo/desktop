import type { CloudKind } from "./types";
import { t, type Key } from "../i18n";

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

/**
 * The sentences that say where the silo folder ends up in the user's
 * account. Whole sentences per provider rather than one place phrase, since
 * the preposition and its article change with the place in most languages.
 */
const CLOUD_PLACE_TEXT: Record<CloudKind, { folders: Key; noSilo: Key; folderHint: Key }> = {
  onedrive: {
    folders: "backup.cloud_folders_onedrive",
    noSilo: "backup.cloud_no_silo_onedrive",
    folderHint: "backup.cloud_folder_hint_onedrive",
  },
  dropbox: {
    folders: "backup.cloud_folders_dropbox",
    noSilo: "backup.cloud_no_silo_dropbox",
    folderHint: "backup.cloud_folder_hint_dropbox",
  },
  "google-drive": {
    folders: "backup.cloud_folders_google",
    noSilo: "backup.cloud_no_silo_google",
    folderHint: "backup.cloud_folder_hint_google",
  },
};

/** "The folders in Apps/SilentSilo on your OneDrive." */
export function cloudFoldersHint(kind: CloudKind): string {
  return t(CLOUD_PLACE_TEXT[kind].folders);
}

/** Setting up from an account that holds no silo yet. */
export function cloudNoSilo(kind: CloudKind): string {
  return t(CLOUD_PLACE_TEXT[kind].noSilo);
}

/** Where a new copy's folder goes, and that the provider sees its name. */
export function cloudFolderHint(kind: CloudKind): string {
  return t(CLOUD_PLACE_TEXT[kind].folderHint);
}

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
  if (!name) return t("backup.cloud_folder_empty");
  if (
    name.length > 100 ||
    /["*:<>?/\\|]/.test(name) ||
    // Control characters, which no file system shows.
    // eslint-disable-next-line no-control-regex
    /[\u0000-\u001f\u007f]/.test(name) ||
    name.startsWith(".") ||
    name.endsWith(".")
  ) {
    return t("backup.cloud_folder_invalid");
  }
  return null;
}
