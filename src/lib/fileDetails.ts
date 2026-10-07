import { whereIs, type BackupTargetView } from "./copies";
import type { FileSyncState } from "./types";

/** One copy as `file_copies` reports it for one file. */
export type FileCopy = {
  id: string;
  /** Null when the content is not on this computer: which copies hold
   * something another device put in backup, this one has not seen. */
  held: boolean | null;
};

export type CopyLine = { id: string; name: string; state: "holds" | "owed" | "unknown" };

/** "JPG file", "File" when the name has no extension. */
export function typeLabel(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return "File";
  return `${name.slice(dot + 1).toUpperCase()} file`;
}

/** Each copy by the name the Copies screen gives it, with what this
 * computer knows of it holding the file. */
export function copyLines(targets: BackupTargetView[], copies: FileCopy[]): CopyLine[] {
  const held = new Map(copies.map((c) => [c.id, c.held]));
  return targets.map((target) => {
    const known = held.get(target.id);
    return {
      id: target.id,
      name: target.label || whereIs(target.config),
      state: known === true ? "holds" : known === false ? "owed" : "unknown",
    };
  });
}

/** Where a file's content is, as a sentence: the row badges in words. */
export function describeSyncState(state: FileSyncState): string {
  if (state === "backed-up") return "On this computer and in backup storage.";
  if (state === "pending") return "On this computer, waiting to be backed up.";
  if (state === "uploading") return "On this computer, uploading to backup storage now.";
  if (state === "downloading") return "In backup storage, downloading to this computer now.";
  if (state === "absent")
    return "Missing. Neither backup storage nor this computer has this file's content. If another device still has the file, open SilentSilo there and sync. Otherwise you can delete it.";
  return "In backup storage only. It downloads when opened.";
}
