import type { StoreConfigView } from "./types";

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
    return "This bucket has versioning but not object lock. SilentSilo will never delete from it, but anything holding the credentials still can, and old versions stay readable. That last part matters for revoking a key: use a bucket with object lock, or credentials without permission to delete.";
  }
  return "This place reports no object lock and no versioning. SilentSilo will never delete from it, but nothing stops anything else with the same credentials. Credentials without permission to delete get you most of the way there.";
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
  const retry =
    target.retry_in > 0 ? `Next attempt in ${describeDuration(target.retry_in)}.` : "";

  if (target.last_success === 0) {
    return {
      health: "never",
      headline: "Not written to yet",
      detail: retry || "The next sync pass will write to it.",
    };
  }

  const age = Math.max(0, nowSeconds - target.last_success);
  const ago = `Last written ${describeDuration(age)} ago.`;
  const backlog =
    behind > 0 ? `${behind} change${behind === 1 ? "" : "s"} not there yet.` : "";

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

  if (behind > 0) {
    return {
      health: "behind",
      headline: `${behind} change${behind === 1 ? "" : "s"} not there yet`,
      detail: retry ? `${ago} ${retry}` : ago,
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
