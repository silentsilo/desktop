import { t } from "../i18n";
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
  if (dot <= 0 || dot === name.length - 1) return t("files.type_file");
  return t("files.type_ext", { ext: name.slice(dot + 1).toUpperCase() });
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
  if (state === "backed-up") return t("files.sync_backed_up_long");
  if (state === "pending") return t("files.sync_pending_long");
  if (state === "uploading") return t("files.sync_uploading_long");
  if (state === "downloading") return t("files.sync_downloading_long");
  if (state === "absent") return t("files.sync_absent_long");
  return t("files.sync_remote_long");
}

/** The same state in two words, with how it reads at a glance. */
export function syncStateShort(state: FileSyncState): {
  label: string;
  tone: "ok" | "wait" | "away" | "bad";
} {
  switch (state) {
    case "backed-up":
      return { label: t("files.sync_backed_up"), tone: "ok" };
    case "pending":
    case "local-only":
      return { label: t("files.sync_waiting"), tone: "wait" };
    case "uploading":
      return { label: t("files.sync_uploading"), tone: "wait" };
    case "downloading":
      return { label: t("files.sync_downloading"), tone: "wait" };
    case "absent":
      return { label: t("files.sync_missing"), tone: "bad" };
    default:
      return { label: t("files.sync_backup_only"), tone: "away" };
  }
}
