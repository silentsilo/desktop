/// Turning a sync pass into the line the user reads.
///
/// Kept out of the panel so it can be tested. The counters alone do not say
/// how a pass went: a target that never opened pushes nothing, a target with
/// nothing to push pushes nothing, and a pass that stood down for one already
/// running pushes nothing either. Reading all three as "up to date" is what
/// this file exists to prevent.

import { t } from "../i18n";
import { formatBytes } from "./format";

/// How one target fared. The pass reports this per target because a target
/// that got nothing is the whole story of the pass.
export type TargetStatus = {
  id: string;
  label: string;
  ops_pushed: number;
  blobs_uploaded: number;
  /// Why this target got nothing. Null means it kept up.
  failed: string | null;
  last_success: number;
  retry_in: number;
  waiting: boolean;
  ops_behind: number;
};

export type SyncReport = {
  /// Which silo the pass was about. The background loop reaches every open
  /// silo, so anything reacting to a report has to know which one it names.
  silo_id?: string;
  configured: boolean;
  ops_pushed: number;
  ops_fetched: number;
  ops_applied: number;
  blobs_uploaded: number;
  blobs_failed: number;
  /// Content a file points at that a copy had lost, put back this pass.
  blobs_restored?: number;
  renamed: string[];
  needs_rebuild: boolean;
  /// The silo's key was rotated from another device and this one was not
  /// kept; it has to rejoin before it can sync again.
  needs_rejoin?: boolean;
  /// The content key in storage does not open with this device's key, while
  /// the records beside it do. No rotation can do that, so the object was
  /// replaced or put back. Rejoining reads the same object, so this must
  /// never be reported as a rejoin.
  key_material_replaced?: boolean;
  compacted: number;
  targets: TargetStatus[];
  /// Another pass was already running and this one stood down.
  skipped?: boolean;
  /// Objects in storage that could not be read, with why.
  unreadable?: string[];
  /// Records waiting because an unreadable object sits below them.
  held_back?: number;
};

/// The backup card's standing line: what is still owed, if anything.
///
/// Records and file content travel separately, records first, so a pass can
/// deliver every record and still leave a file behind, from a queue it has
/// not reached or an upload that failed. The card counted records alone and
/// said "Everything is backed up" over content that exists on this computer
/// only. Content leads the sentence: a record that has not gone out is a
/// name and a size, a blob that has not gone out is the file itself.
export function backupHeadline(
  pendingOps: number,
  unsyncedCount: number,
  unsyncedBytes: number,
  lastSyncAt: number | null,
): string {
  if (unsyncedCount > 0) {
    const count = t("app.sync_n_files", { count: unsyncedCount });
    const files =
      unsyncedBytes > 0
        ? t("app.sync_files_size", { files: count, size: formatBytes(unsyncedBytes) })
        : count;
    // One phrase for pending work everywhere it is shown: the sidebar, the
    // status bar and this card used to say it three different ways.
    return pendingOps > 0
      ? t("app.headline_files_changes", {
          files,
          changes: t("app.sync_n_changes", { count: pendingOps }),
        })
      : t("app.headline_files", { count: unsyncedCount, files });
  }
  if (pendingOps > 0) {
    return t("app.headline_changes", { count: pendingOps });
  }
  return lastSyncAt ? t("app.headline_synced") : t("app.headline_connected");
}

export type Status =
  | { kind: "idle" }
  | { kind: "busy"; message: string }
  | { kind: "ok"; message: string }
  | { kind: "error"; message: string };

/// One line summarising what a pass actually did, rather than a bare "done".
export function describeSync(r: SyncReport): string {
  if (r.needs_rejoin) {
    return t("app.sync_needs_rejoin");
  }
  // Never the rejoin wording: rejoining reads the same content key, so it
  // would fail on the same object and leave the user going round a loop.
  if (r.key_material_replaced) {
    return t("app.sync_key_replaced");
  }
  if (r.needs_rebuild) {
    return t("app.sync_needs_rebuild");
  }
  const parts: string[] = [];
  if (r.ops_pushed > 0) parts.push(t("app.sync_sent", { count: r.ops_pushed }));
  if (r.ops_applied > 0) parts.push(t("app.sync_received", { count: r.ops_applied }));
  if (r.blobs_uploaded > 0) parts.push(t("app.sync_backed_up", { count: r.blobs_uploaded }));
  if (r.blobs_failed > 0) parts.push(t("app.sync_failed_retry", { count: r.blobs_failed }));
  const restored = r.blobs_restored ?? 0;
  if (restored > 0) parts.push(t("app.sync_restored", { count: restored }));
  const unreadable = r.unreadable?.length ?? 0;
  if (unreadable > 0) parts.push(t("app.sync_unreadable", { count: unreadable }));
  // Housekeeping, mentioned rather than announced: the user did not ask for
  // it and nothing of theirs changed.
  if (r.compacted > 0) parts.push(t("app.sync_compacted", { count: r.compacted }));
  if (parts.length > 0) return parts.join(", ");

  // A pass that stood down reached nothing, so its zeroes say nothing about
  // whether the copies are current. Reporting them as "up to date" is how a
  // backup still uploading gets announced as finished.
  if (r.skipped) return t("app.sync_already_running");

  // Nothing moved. That is only good news when every target was reachable:
  // a pass where each one failed produces exactly these zeroes, and saying
  // "up to date" over it turns a total failure into a green tick.
  const behind = (r.targets ?? []).some((t) => t.ops_behind > 0);
  return behind ? t("app.sync_nothing_sent") : t("app.sync_up_to_date");
}

/// The pass as a status, so a failure reads as one.
///
/// The reason a target got nothing lives per target, which is why it is read
/// here rather than inferred from the totals.
export function syncOutcome(r: SyncReport, renamed: string): Status {
  // Not a per-target failure: nothing was attempted, and the message is the
  // whole outcome.
  if (r.needs_rejoin || r.key_material_replaced) {
    return { kind: "error", message: describeSync(r) };
  }
  const targets = r.targets ?? [];
  const failed = targets.filter((t) => t.failed);
  const waiting = targets.filter((t) => !t.failed && t.waiting);

  if (failed.length > 0) {
    // Labelled only when there is more than one, since naming the single
    // target someone is looking at reads as bureaucracy.
    const detail =
      failed.length === 1 && targets.length === 1
        ? failed[0]!.failed
        : failed.map((t) => `${t.label}: ${t.failed}`).join(" ");
    const partial = r.ops_pushed > 0 ? `${describeSync(r)} ` : "";
    return { kind: "error", message: `${partial}${detail}` };
  }

  if (waiting.length > 0 && r.ops_pushed === 0) {
    const when = Math.max(...waiting.map((t) => t.retry_in));
    return {
      kind: "ok",
      message: t("app.sync_waiting", { minutes: Math.ceil(when / 60) }),
    };
  }

  return { kind: "ok", message: describeSync(r) + renamed };
}
