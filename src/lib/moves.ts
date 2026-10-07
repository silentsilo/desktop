import { t } from "../i18n";
import type { VaultEntry } from "./types";

/** Where a move goes: a folder by id (a row in the listing) or by path (a
 * segment of the address bar), and what to call it in messages. */
export type MoveDestination = { id?: string; path?: string; label: string };

export type MoveReport = {
  moved: number;
  /** Names left where they were: the destination had one already. */
  skipped: string[];
  failed: { name: string; reason: string }[];
};

export const moveItems = (entries: VaultEntry[]) =>
  entries.map((entry) => ({ kind: entry.kind, id: entry.id }));

/**
 * Whether `moving` may be dropped on the folder at `target`: not on a folder
 * being moved, not inside one, and not on the folder they are already in.
 * Core refuses the first two anyway; this keeps the drop target from
 * lighting up for them.
 */
export function canMoveTo(
  moving: VaultEntry[],
  target: { id?: string; path: string },
  currentPath: string,
): boolean {
  if (moving.length === 0) return false;
  if (target.path === currentPath) return false;
  return moving.every((entry) => {
    if (entry.kind !== "folder") return true;
    if (target.id !== undefined && target.id === entry.id) return false;
    return target.path !== entry.path && !target.path.startsWith(`${entry.path}/`);
  });
}

/** What the toast says after a move, and whether it is an error. */
export function moveSummary(report: MoveReport, destination: string): { text: string; error: boolean } {
  const parts: string[] = [];
  if (report.moved > 0) {
    parts.push(t("files.moved", { count: report.moved, destination }));
  }
  if (report.skipped.length > 0) {
    parts.push(
      report.skipped.length === 1
        ? t("files.skipped_one", { name: report.skipped[0]!, destination })
        : t("files.skipped_many", { count: report.skipped.length, destination }),
    );
  }
  if (report.failed.length > 0) {
    const first = report.failed[0]!;
    const reason = first.reason.replace(/\.$/, "");
    parts.push(
      report.failed.length === 1
        ? t("files.failed_one", { name: first.name, reason })
        : t("files.failed_many", { count: report.failed.length, name: first.name, reason }),
    );
  }
  if (parts.length === 0) parts.push(t("files.nothing_moved"));
  return { text: parts.join(" "), error: report.failed.length > 0 };
}
