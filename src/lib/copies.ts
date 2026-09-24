import { formatBytes } from "./format";
import type { SeedProgress, StoreConfigView } from "./types";

/**
 * One place this silo backs up to, as the backend reports it.
 *
 * The three numbers are what turn a list of connections into a list of
 * copies: a copy is not on or off, it has an age and a backlog.
 */
export type BackupTargetView = {
  id: string;
  label: string;
  config: StoreConfigView;
  primary: boolean;
  /** Unix seconds of the last pass this target took everything, 0 if never. */
  last_success: number;
  /** Changes this target has not received. */
  ops_behind: number;
  /** Files this target does not have yet. Absent from older backends. */
  blobs_behind?: number;
  /** Seconds until sync tries again, 0 when it is due now. */
  retry_in: number;
  /** The app never sends this one a delete, so it grows for ever. */
  archive: boolean;
};

/** What a place reports about resisting deletion, asked before it is trusted. */
export type Protection = {
  versioning: boolean;
  object_lock: boolean;
};

/**
 * What to tell someone who has just asked for a place to be append-only.
 *
 * Empty when there is nothing to warn about. The two cases are different and
 * both matter: object lock means the storage itself refuses a delete, while
 * versioning alone means a delete is accepted and the old version stays
 * readable, so revoking a key looks like it worked when it did not.
 */
export function protectionWarning(p: Protection | null, archive: boolean): string {
  if (!archive || !p) return "";
  if (p.object_lock) return "";
  if (p.versioning) {
    return "This bucket has versioning but no object lock. SilentSilo does not delete from it, but anyone with the access key can, and old versions stay readable after revoking a key. Use a bucket with object lock, or an access key that cannot delete.";
  }
  return "This backup storage reports no object lock and no versioning. SilentSilo does not delete from it, but anyone with the same sign-in details can. Use sign-in details that cannot delete.";
}

/**
 * How worried to be about one copy.
 *
 * "never" is separate from "stale" because they need different sentences: a
 * copy that has never been written is a setup problem, and one that was fine
 * a month ago is a disk somebody has to go and plug in.
 */
export type CopyHealth = "current" | "behind" | "stale" | "never";

/** A week without a successful write is when a copy stops counting as one. */
export const STALE_AFTER_SECONDS = 7 * 24 * 60 * 60;

/**
 * A duration as a person would say it, coarse on purpose.
 *
 * "47 days ago" is the sentence someone can act on. "1,139 hours ago" is the
 * same fact with the decision removed.
 */
export function describeDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return s <= 1 ? "a moment" : `${s} seconds`;
  const minutes = Math.round(s / 60);
  if (minutes < 60) return minutes === 1 ? "a minute" : `${minutes} minutes`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return hours === 1 ? "an hour" : `${hours} hours`;
  const days = Math.round(hours / 24);
  if (days < 30) return days === 1 ? "a day" : `${days} days`;
  const months = Math.round(days / 30);
  if (months < 12) return months === 1 ? "a month" : `${months} months`;
  const years = Math.round(months / 12);
  return years === 1 ? "a year" : `${years} years`;
}

export type CopyState = {
  health: CopyHealth;
  /** The one line that goes next to the name. */
  headline: string;
  /** The supporting fact, or empty when the headline already said it. */
  detail: string;
};

/**
 * What to say about one copy, given the clock.
 *
 * `now` is passed in rather than read here so this is testable and so a list
 * of copies renders against a single instant instead of drifting row by row.
 */
export function copyState(target: BackupTargetView, nowSeconds: number): CopyState {
  const behind = target.ops_behind;
  const files = target.blobs_behind ?? 0;
  const retry =
    target.retry_in > 0 ? `Next attempt in ${describeDuration(target.retry_in)}.` : "";

  if (target.last_success === 0) {
    return {
      health: "never",
      headline: "Not written to yet",
      detail: retry || "The next sync will write to it.",
    };
  }

  const age = Math.max(0, nowSeconds - target.last_success);
  const ago = `Last written ${describeDuration(age)} ago.`;
  const backlog = [
    files > 0 ? `${files} file${files === 1 ? "" : "s"} not there yet.` : "",
    behind > 0 ? `${behind} change${behind === 1 ? "" : "s"} not there yet.` : "",
  ]
    .filter(Boolean)
    .join(" ");

  // Once a copy has gone stale the age is the headline, whatever the backlog
  // says. A disk unplugged since spring being "3 changes behind" invites
  // reading it as nearly fine, and the small number is the wrong end of the
  // problem.
  if (age >= STALE_AFTER_SECONDS) {
    return {
      health: "stale",
      headline: `Last written ${describeDuration(age)} ago`,
      detail: [backlog, retry].filter(Boolean).join(" "),
    };
  }

  // Files lead: a change that has not gone out is a name and a size, a
  // file that has not gone out is the thing itself.
  if (files > 0 || behind > 0) {
    return {
      health: "behind",
      headline: files > 0
        ? `${files} file${files === 1 ? "" : "s"} not there yet`
        : `${behind} change${behind === 1 ? "" : "s"} not there yet`,
      detail: [files > 0 && behind > 0 ? `${behind} change${behind === 1 ? "" : "s"} too.` : "", ago, retry]
        .filter(Boolean)
        .join(" "),
    };
  }

  return { health: "current", headline: "Up to date", detail: ago };
}

/**
 * How many of the listed places actually hold a copy right now.
 *
 * Counted rather than assumed from the number of targets: 3-2-1 is a practice
 * and the whole point of the panel is that believing you have three copies
 * when one has been unplugged since spring is the failure mode.
 */
export function currentCopies(targets: BackupTargetView[], nowSeconds: number): number {
  return targets.filter((t) => copyState(t, nowSeconds).health === "current").length;
}

/**
 * The line under a running fill.
 *
 * Both counts, because neither alone is enough: the object count stands
 * still for the minutes one large blob takes, and the bytes alone hide that
 * a thousand small records are what is left.
 */
export function seedHeadline(p: SeedProgress): string {
  const objects = `${p.objects_done} of ${p.objects_total} item${
    p.objects_total === 1 ? "" : "s"
  }`;
  if (p.bytes_total <= 0) return `Copying: ${objects}.`;
  return `Copying: ${objects}, ${formatBytes(p.bytes_done)} of ${formatBytes(p.bytes_total)}.`;
}

/**
 * The same thing in the width of a button, where one number is all there is
 * room for. The bytes, because that is the one that moves while a large
 * object goes across, which is the whole reason they are reported.
 */
export function seedLabel(p: SeedProgress): string {
  if (p.bytes_total <= 0) return `${p.objects_done} of ${p.objects_total}…`;
  return `${formatBytes(p.bytes_done)} of ${formatBytes(p.bytes_total)}…`;
}

/**
 * How far along the bar is, 0 to 100.
 *
 * Bytes wherever there are any: they move during a large object, and an
 * object skipped or failed is credited whole when it is done with, so the
 * bar still ends where the object count does.
 */
export function seedPercent(p: SeedProgress): number {
  const done = p.bytes_total > 0 ? p.bytes_done : p.objects_done;
  const total = p.bytes_total > 0 ? p.bytes_total : p.objects_total;
  if (total <= 0) return 0;
  return Math.min(100, Math.max(0, (done / total) * 100));
}
