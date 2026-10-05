import { trimmed, type HistoryPolicy } from "./entryHistory";
import { applyTotp } from "./passwordCsv";
import { appendNotes } from "./passwordImport";
import type {
  CustomField,
  HistoryVersion,
  PasswordAttachment,
  PasswordEntry,
} from "./types";

/** One field a KeePass entry names beyond the standard five. */
export type KdbxField = { name: string; value: string; protected: boolean };

/** What one version of a KeePass entry says, as `passwords_read_kdbx` sends it. */
export type KdbxVersion = {
  title: string;
  username: string;
  password: string;
  url: string;
  notes: string;
  otp: string | null;
  fields: KdbxField[];
  modified: number | null;
};

export type KdbxEntry = KdbxVersion & {
  /** Group names from the top, the root left out. */
  group: string[];
  tags: string[];
  created: number | null;
  /** Already encrypted into the silo. */
  attachments: PasswordAttachment[];
  /** Earlier versions, newest first. */
  history: KdbxVersion[];
};

/** The tag this app's own export writes for a starred entry. */
const FAVORITE_TAG = "Favorite";

function fieldsOf(version: KdbxVersion): CustomField[] {
  return version.fields.map((f) => ({ name: f.name, value: f.value, hidden: f.protected }));
}

/** What a version says, in the shape of an entry. A TOTP the importers cannot
 * read goes into notes, as from a CSV. */
function contentOf(version: KdbxVersion): Partial<PasswordEntry> {
  const out: PasswordEntry = {
    id: "",
    service: version.title,
    username: version.username,
    password: version.password,
    url: version.url,
    notes: version.notes,
    category: "",
    created_at: 0,
    updated_at: 0,
    type: "login",
  };
  if (version.otp && !applyTotp(out, version.otp)) {
    out.notes = appendNotes(out.notes, [`Two-factor secret: ${version.otp}`]);
  }
  const fields = fieldsOf(version);
  if (fields.length > 0) out.fields = fields;
  const { id: _id, category: _category, created_at: _created, updated_at: _updated, ...rest } = out;
  return rest;
}

export type KdbxImport = {
  entries: PasswordEntry[];
  /** Entries with nothing in them: no name, no user name, no password. */
  skipped: PasswordEntry[];
};

/**
 * The silo's entries for a KeePass database's. A group path becomes the
 * category, "Work / Servers", so two groups of one name under different
 * parents stay apart; tags other than this app's own star go into notes,
 * which is the only place they would still be read.
 */
export function kdbxToEntries(
  read: KdbxEntry[],
  policy: HistoryPolicy,
  now: () => number = Date.now,
): KdbxImport {
  const entries: PasswordEntry[] = [];
  const skipped: PasswordEntry[] = [];
  for (const kp of read) {
    const at = kp.modified ?? kp.created ?? now();
    const otherTags = kp.tags.filter((t) => t !== FAVORITE_TAG);
    const entry: PasswordEntry = {
      id: crypto.randomUUID(),
      service: "",
      username: "",
      password: "",
      url: "",
      notes: "",
      category: kp.group.join(" / ") || "General",
      created_at: kp.created ?? at,
      updated_at: at,
      ...contentOf(kp),
    };
    if (!entry.service) entry.service = kp.url || kp.username || "Untitled";
    if (otherTags.length > 0) entry.notes = appendNotes(entry.notes, [`Tags: ${otherTags.join(", ")}`]);
    if (kp.tags.includes(FAVORITE_TAG)) entry.favorite = true;
    if (kp.attachments.length > 0) entry.attachments = kp.attachments;

    const history: HistoryVersion[] = kp.history.map((v) => ({
      ...contentOf(v),
      saved_at: v.modified ?? 0,
    }));
    const kept = trimmed(history, policy);
    if (kept.length > 0) entry.history = kept;

    const empty = !kp.title && !kp.username && !kp.password && kp.attachments.length === 0;
    (empty ? skipped : entries).push(entry);
  }
  return { entries, skipped };
}

/** Every attachment the given entries carry, by blob id: what an import that
 * is cancelled, or drops an entry as a duplicate, has to delete again. */
export function attachmentBlobs(entries: PasswordEntry[]): string[] {
  return entries.flatMap((e) => (e.attachments ?? []).map((a) => a.blob_id));
}
