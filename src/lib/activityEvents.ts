import type { AuditEntry } from "./types";

/** The groups the Activity page filters by, each a set of event codes. */
export type ActivityKind = "access" | "secrets" | "entries" | "files" | "security";

export const KIND_LABELS: Record<ActivityKind, string> = {
  access: "Unlocks",
  secrets: "Secrets",
  entries: "Entries",
  files: "Files",
  security: "Keys and settings",
};

/** The event codes in each group; the numbers are core's (`events.rs`). */
export const KIND_CODES: Record<ActivityKind, number[]> = {
  access: [1, 2, 3],
  secrets: [10, 11, 12, 13, 14, 15],
  entries: [20, 21, 22, 23, 24, 40, 41],
  files: [30, 31, 32, 33, 34],
  security: [50, 51, 52, 53, 54, 60, 61, 62, 63, 70, 71],
};

/** How an event reads: worth a second look, or ordinary. */
export type ActivityTone = "plain" | "notice";

/** Which picture goes with an event; the view maps it to an icon. */
export type ActivityIcon =
  | "unlock"
  | "lock"
  | "refused"
  | "show"
  | "copy"
  | "code"
  | "browser"
  | "app"
  | "ssh"
  | "create"
  | "edit"
  | "delete"
  | "restore"
  | "import"
  | "export"
  | "file"
  | "file-out"
  | "file-add"
  | "trash"
  | "key"
  | "recovery"
  | "rotate"
  | "log"
  | "device"
  | "repair"
  | "other";

export type DescribedEvent = {
  icon: ActivityIcon;
  tone: ActivityTone;
  /** The sentence around the object it names, which is shown strong. */
  before: string;
  object: string;
  after: string;
  /** What else the event carries, already in words. */
  details: string[];
};

/** How a key's kind reads. A security key and Windows Hello are both
 * `fido2`, so that one says nothing. */
const KEY_KINDS: Record<string, string> = {
  fido2: "",
  "secure-enclave": "the Mac's built-in key",
  "android-keystore": "the phone's built-in key",
};

function text(value: unknown): string {
  if (value === undefined || value === null) return "";
  return typeof value === "string" ? value : String(value);
}

function count(value: unknown, one: string, many: string): string {
  const n = Number(value);
  if (!Number.isFinite(n) || n <= 0) return many;
  return `${n} ${n === 1 ? one : many}`;
}

/** The details an event's own sentence did not already use. */
function rest(x: Record<string, unknown>, used: string[]): string[] {
  return Object.keys(x)
    .filter((k) => !used.includes(k) && x[k] !== undefined && x[k] !== "")
    .filter((k) => !(k === "kind" && KEY_KINDS[text(x[k])] === ""))
    .map((k) => {
      const v = x[k];
      if (k === "site") return `on ${text(v)}`;
      if (k === "host") return `server ${text(v)}`;
      if (k === "program") return `by ${text(v)}`;
      if (k === "format") return `as ${text(v).toUpperCase()}`;
      if (k === "entry") return `in ${text(v)}`;
      if (k === "kind") return KEY_KINDS[text(v)] ?? text(v);
      return typeof v === "object" ? `${k}: ${JSON.stringify(v)}` : `${k}: ${text(v)}`;
    });
}

/** One event as the Activity page says it. A code this build does not know
 * keeps the name core gave it. */
export function describe(entry: AuditEntry): DescribedEvent {
  const x = entry.x ?? {};
  const l = entry.l ?? "";
  const make = (
    icon: ActivityIcon,
    before: string,
    object = "",
    after = "",
    used: string[] = [],
    tone: ActivityTone = "plain",
  ): DescribedEvent => ({
    icon,
    tone,
    before,
    object,
    after,
    details: rest(x, used),
  });

  switch (entry.c) {
    case 1: {
      if (x.key) return make("unlock", "Unlocked with ", text(x.key), "", ["key"]);
      if (x.by) return make("unlock", "Unlocked with the ", text(x.by), "", ["by"]);
      return make("unlock", "Unlocked");
    }
    case 2:
      return make("lock", "Locked");
    case 3:
      return make("refused", "An unlock was refused", "", "", [], "notice");
    case 10:
      return make("show", "Showed ", l);
    case 11:
      return make("copy", `Copied the ${text(x.field) || "secret"} of `, l, "", ["field"]);
    case 12:
      return make("code", "Copied a one-time code from ", l, "", ["field"]);
    case 13:
      return make("browser", "Filled ", l, " in the browser");
    case 14:
      return make("app", "Filled ", l, " in an app");
    case 15:
      return make(
        "ssh",
        "Signed with ",
        l,
        x.for === "git" ? " for Git" : text(x.for) ? ` for ${text(x.for)}` : "",
        ["for"],
      );
    case 20:
      return make("create", "Created ", l);
    case 21:
      return make("edit", "Edited ", l);
    case 22:
      return make("delete", "Deleted ", l);
    case 23:
      return make("restore", "Restored an earlier version of ", l);
    case 24:
      return make("delete", "Cleared the history of ", l);
    case 30:
      return make("file", "Opened ", l);
    case 31:
      return l
        ? make("file-out", "Saved ", l, " outside the silo", ["files"])
        : make("file-out", "Saved ", count(x.files, "file", "files"), " outside the silo", [
            "files",
          ]);
    case 32:
      return l
        ? make("file-add", "Added ", l, "", ["count"])
        : make("file-add", "Added ", count(x.count, "file", "files"), "", ["count"]);
    case 33:
      return make("trash", "Moved ", l, " to the trash");
    case 34:
      if (x.what === "trash") return make("trash", "Emptied the trash", "", "", ["what"]);
      return l
        ? make("trash", "Deleted ", l, " for good", ["count"])
        : make("trash", "Deleted ", count(x.count, "item", "items"), " for good", ["count"]);
    case 40: {
      // What it came from, as the window named it: "KeePass", "Chrome/Edge".
      const imported = make("import", "Imported ", count(x.count, "password", "passwords"), "", [
        "count",
        "format",
      ]);
      if (text(x.format)) imported.details.unshift(`from ${text(x.format)}`);
      return imported;
    }
    case 41:
      return make("export", "Exported ", count(x.count, "password", "passwords"), "", ["count"]);
    case 50:
      return make("key", "Added the key ", l, "", [], "notice");
    case 51:
      return make("key", "Removed the key ", l, "", [], "notice");
    case 52:
      return make("recovery", "Used the recovery code", "", "", [], "notice");
    case 53:
      return x.now === "off"
        ? make("recovery", "Turned the recovery code off", "", "", ["now"], "notice")
        : make("recovery", "Made a new recovery code", "", "", ["now"], "notice");
    case 54:
      return make("rotate", "Replaced the encryption key", "", "", ["kept"], "notice");
    case 60:
      return x.for === "organisation"
        ? make("log", "The organisation's activity log started", "", "", ["for"], "notice")
        : make("log", "Activity started", "", "", ["for"]);
    case 61:
      return x.for === "organisation"
        ? make("log", "Activity moved to the organisation's log", "", "", ["for"], "notice")
        : make("log", "Activity stopped", "", "", ["for"], "notice");
    case 62: {
      const days = Number(x.days);
      return Number.isFinite(days) && days > 0
        ? make("log", "Activity is now kept for ", count(days, "day", "days"), "", ["days"])
        : make("log", "Activity is now kept with no time limit", "", "", ["days"]);
    }
    case 63:
      return make("log", "Removed old activity", "", "", ["count"]);
    case 70:
      return make("device", "A device joined", l ? ": " : "", l);
    case 71:
      return make("repair", "Repaired the silo");
    default:
      return make("other", l ? `${entry.what}: ` : entry.what, l);
  }
}

/** The heading a day's events go under: Today, Yesterday, or the date. */
export function dayLabel(ms: number, now: number = Date.now()): string {
  const day = new Date(ms);
  const today = new Date(now);
  const start = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const diff = Math.round((start(today) - start(day)) / 86_400_000);
  if (diff === 0) return "Today";
  if (diff === 1) return "Yesterday";
  return day.toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
    ...(day.getFullYear() === today.getFullYear() ? {} : { year: "numeric" }),
  });
}

/** Consecutive events of the same day, in the order given. */
export function byDay<T extends { t: number }>(
  events: T[],
  now: number = Date.now(),
): { label: string; events: T[] }[] {
  const groups: { label: string; events: T[] }[] = [];
  for (const event of events) {
    const label = dayLabel(event.t, now);
    const last = groups[groups.length - 1];
    if (last && last.label === label) last.events.push(event);
    else groups.push({ label, events: [event] });
  }
  return groups;
}
