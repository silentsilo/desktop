import type { AuditEntry } from "./types";
import { dateLocale, t, type Key } from "../i18n";

/** The groups the Activity page filters by, each a set of event codes. */
export type ActivityKind = "access" | "secrets" | "entries" | "files" | "security";

/** Getters, so a label read at render is in the language in use. */
export const KIND_LABELS: Record<ActivityKind, string> = {
  get access() {
    return t("dlg.activity_kind_access");
  },
  get secrets() {
    return t("dlg.activity_kind_secrets");
  },
  get entries() {
    return t("dlg.activity_kind_entries");
  },
  get files() {
    return t("nav.files");
  },
  get security() {
    return t("dlg.activity_kind_security");
  },
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
const KEY_KINDS: Record<string, Key | ""> = {
  fido2: "",
  "secure-enclave": "dlg.activity_key_mac",
  "android-keystore": "dlg.activity_key_phone",
};

/** What a copied field is called, for the fields the app names. Anything
 * else was recorded as its label and is shown as it is. */
const FIELDS: Record<string, Key> = {
  password: "dlg.activity_field_password",
  "card number": "dlg.activity_field_card_number",
  notes: "dlg.activity_field_notes",
  email: "dlg.activity_field_email",
  "public key": "dlg.activity_field_public_key",
  "private key": "dlg.activity_field_private_key",
};

function text(value: unknown): string {
  if (value === undefined || value === null) return "";
  return typeof value === "string" ? value : String(value);
}

/** A count as the object of a sentence: "3 files", or "files" when the
 * log has no usable number. `n` picks the sentence's own plural form, NaN
 * landing on "other". */
function count(value: unknown, counted: Key, bare: Key): { object: string; n: number } {
  const n = Number(value);
  if (!Number.isFinite(n) || n <= 0) return { object: t(bare), n: Number.NaN };
  return { object: t(counted, { count: n }), n };
}

/** A sentence with `{object}` in it, cut around the object, which the view
 * shows strong. */
function around(
  sentence: string,
  object: string,
): { before: string; object: string; after: string } {
  const at = sentence.indexOf("{object}");
  if (at < 0) return { before: sentence, object: "", after: "" };
  return { before: sentence.slice(0, at), object, after: sentence.slice(at + "{object}".length) };
}

/** The details an event's own sentence did not already use. */
function rest(x: Record<string, unknown>, used: string[]): string[] {
  return Object.keys(x)
    .filter((k) => !used.includes(k) && x[k] !== undefined && x[k] !== "")
    .filter((k) => !(k === "kind" && KEY_KINDS[text(x[k])] === ""))
    .map((k) => {
      const v = x[k];
      if (k === "site") return t("dlg.activity_detail_site", { site: text(v) });
      if (k === "host") return t("dlg.activity_detail_host", { host: text(v) });
      if (k === "program") return t("dlg.activity_detail_program", { program: text(v) });
      if (k === "format") return t("dlg.activity_detail_format", { format: text(v).toUpperCase() });
      if (k === "entry") return t("dlg.activity_detail_entry", { entry: text(v) });
      if (k === "kind") {
        const known = KEY_KINDS[text(v)];
        return known ? t(known) : text(v);
      }
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
  /** A whole sentence from the catalog, its `{object}` shown strong. */
  const say = (
    icon: ActivityIcon,
    key: Key,
    object: string,
    used: string[] = [],
    tone: ActivityTone = "plain",
    params: Record<string, string | number> = {},
  ): DescribedEvent => {
    const parts = around(t(key, params), object);
    return make(icon, parts.before, parts.object, parts.after, used, tone);
  };
  /** A sentence about a counted object, its verb agreeing with the count. */
  const sayCount = (
    icon: ActivityIcon,
    key: Key,
    counted: { object: string; n: number },
    used: string[],
  ): DescribedEvent => say(icon, key, counted.object, used, "plain", { count: counted.n });

  switch (entry.c) {
    case 1: {
      if (x.key) return say("unlock", "dlg.activity_unlocked_with", text(x.key), ["key"]);
      if (x.by) {
        const by =
          text(x.by) === "recovery code" ? t("dlg.activity_by_recovery_code") : text(x.by);
        return say("unlock", "dlg.activity_unlocked_with_the", by, ["by"]);
      }
      return make("unlock", t("dlg.activity_unlocked"));
    }
    case 2:
      return make("lock", t("dlg.activity_locked"));
    case 3:
      return make("refused", t("dlg.activity_refused"), "", "", [], "notice");
    case 10:
      return say("show", "dlg.activity_showed", l);
    case 11: {
      const field = text(x.field);
      const known = FIELDS[field];
      return say("copy", "dlg.activity_copied_field", l, ["field"], "plain", {
        field: known ? t(known) : field || t("dlg.activity_field_secret"),
      });
    }
    case 12:
      return say("code", "dlg.activity_code_copied", l, ["field"]);
    case 13:
      return say("browser", "dlg.activity_filled_browser", l);
    case 14:
      return say("app", "dlg.activity_filled_app", l);
    case 15:
      if (x.for === "git") return say("ssh", "dlg.activity_signed_git", l, ["for"]);
      if (text(x.for)) {
        return say("ssh", "dlg.activity_signed_for", l, ["for"], "plain", {
          purpose: text(x.for),
        });
      }
      return say("ssh", "dlg.activity_signed", l, ["for"]);
    case 20:
      return say("create", "dlg.activity_created", l);
    case 21:
      return say("edit", "dlg.activity_edited", l);
    case 22:
      return say("delete", "dlg.activity_deleted", l);
    case 23:
      return say("restore", "dlg.activity_restored_version", l);
    case 24:
      return say("delete", "dlg.activity_cleared_history", l);
    case 30:
      return say("file", "dlg.activity_opened", l);
    case 31:
      return l
        ? say("file-out", "dlg.activity_saved_out", l, ["files"])
        : sayCount(
            "file-out",
            "dlg.activity_saved_out_count",
            count(x.files, "dlg.activity_n_files", "dlg.activity_files"),
            ["files"],
          );
    case 32:
      return l
        ? say("file-add", "dlg.activity_added", l, ["count"])
        : sayCount(
            "file-add",
            "dlg.activity_added_count",
            count(x.count, "dlg.activity_n_files", "dlg.activity_files"),
            ["count"],
          );
    case 33:
      return say("trash", "dlg.activity_moved_trash", l);
    case 34:
      if (x.what === "trash") {
        return make("trash", t("dlg.activity_emptied_trash"), "", "", ["what"]);
      }
      return l
        ? say("trash", "dlg.activity_deleted_for_good", l, ["count"])
        : sayCount(
            "trash",
            "dlg.activity_deleted_for_good_count",
            count(x.count, "dlg.activity_n_items", "dlg.activity_items"),
            ["count"],
          );
    case 40: {
      // What it came from, as the window named it: "KeePass", "Chrome/Edge".
      const imported = sayCount(
        "import",
        "dlg.activity_imported",
        count(x.count, "dlg.activity_n_passwords", "dlg.activity_passwords"),
        ["count", "format"],
      );
      if (text(x.format)) {
        imported.details.unshift(t("dlg.activity_detail_from", { format: text(x.format) }));
      }
      return imported;
    }
    case 41:
      return sayCount(
        "export",
        "dlg.activity_exported",
        count(x.count, "dlg.activity_n_passwords", "dlg.activity_passwords"),
        ["count"],
      );
    case 50:
      return say("key", "dlg.activity_key_added", l, [], "notice");
    case 51:
      return say("key", "dlg.activity_key_removed", l, [], "notice");
    case 52:
      return make("recovery", t("dlg.activity_recovery_used"), "", "", [], "notice");
    case 53:
      return x.now === "off"
        ? make("recovery", t("dlg.activity_recovery_off"), "", "", ["now"], "notice")
        : make("recovery", t("dlg.activity_recovery_new"), "", "", ["now"], "notice");
    case 54:
      return make("rotate", t("dlg.activity_key_rotated"), "", "", ["kept"], "notice");
    case 60:
      return x.for === "organisation"
        ? make("log", t("dlg.activity_org_started"), "", "", ["for"], "notice")
        : make("log", t("dlg.activity_started"), "", "", ["for"]);
    case 61:
      return x.for === "organisation"
        ? make("log", t("dlg.activity_org_moved"), "", "", ["for"], "notice")
        : make("log", t("dlg.activity_stopped"), "", "", ["for"], "notice");
    case 62: {
      const days = Number(x.days);
      return Number.isFinite(days) && days > 0
        ? say("log", "dlg.activity_kept_for", t("dlg.activity_n_days", { count: days }), [
            "days",
          ])
        : make("log", t("dlg.activity_kept_forever"), "", "", ["days"]);
    }
    case 63:
      return make("log", t("dlg.activity_pruned"), "", "", ["count"]);
    case 70:
      return l
        ? say("device", "dlg.activity_device_joined_named", l)
        : make("device", t("dlg.activity_device_joined"));
    case 71:
      return make("repair", t("dlg.activity_repaired"));
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
  if (diff === 0) return t("dlg.activity_today");
  if (diff === 1) return t("dlg.activity_yesterday");
  return day.toLocaleDateString(dateLocale(), {
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
