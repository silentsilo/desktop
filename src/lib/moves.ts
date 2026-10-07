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
    parts.push(
      report.moved === 1 ? `Moved 1 item to ${destination}.` : `Moved ${report.moved} items to ${destination}.`,
    );
  }
  if (report.skipped.length > 0) {
    parts.push(
      report.skipped.length === 1
        ? `Left "${report.skipped[0]}" where it was: ${destination} has one already.`
        : `Left ${report.skipped.length} items where they were: ${destination} has their names already.`,
    );
  }
  if (report.failed.length > 0) {
    const first = report.failed[0]!;
    parts.push(
      report.failed.length === 1
        ? `"${first.name}" did not move: ${first.reason.replace(/\.$/, "")}.`
        : `${report.failed.length} items did not move. The first, "${first.name}": ${first.reason.replace(/\.$/, "")}.`,
    );
  }
  if (parts.length === 0) parts.push("Nothing moved: everything is there already.");
  return { text: parts.join(" "), error: report.failed.length > 0 };
}
