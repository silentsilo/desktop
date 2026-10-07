import type { HistoryVersion, PasswordEntry } from "./types";

/** How many versions to keep: a number, or as many as fit the budget. */
export type HistoryPolicy = number | "fit";

export const HISTORY_POLICIES: readonly HistoryPolicy[] = [10, 30, "fit"];
export const DEFAULT_HISTORY_POLICY: HistoryPolicy = 10;

/**
 * Most a history may take, as JSON. The whole entry is one record, and core
 * refuses an entry over 512 KB so that readers before 1.4 can still read it;
 * this leaves the rest for the entry itself.
 */
export const HISTORY_BYTES = 256 * 1024;

/** Kept on the entry rather than in a version: who it is, where it is filed,
 * how it is guarded, what is attached, the history itself, and a version's
 * own date. */
const NOT_VERSIONED = new Set([
  "id",
  "history",
  "saved_at",
  "attachments",
  "passkey",
  "favorite",
  "category",
  "created_at",
  "updated_at",
  "require_reauth",
]);

function isEmpty(value: unknown): boolean {
  return (
    value === undefined ||
    value === null ||
    value === "" ||
    (Array.isArray(value) && value.length === 0)
  );
}

/** What an entry says, with empty values left out, so a field that went
 * from absent to empty is no change. */
function contentOf(entry: Partial<PasswordEntry>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(entry)) {
    if (!NOT_VERSIONED.has(key) && !isEmpty(value)) out[key] = value;
  }
  return out;
}

function sameContent(a: Partial<PasswordEntry>, b: Partial<PasswordEntry>): boolean {
  const left = contentOf(a);
  const right = contentOf(b);
  const keys = Object.keys(left);
  if (keys.length !== Object.keys(right).length) return false;
  return keys.every((key) => JSON.stringify(left[key]) === JSON.stringify(right[key]));
}

/** Bytes of `value` as the stored JSON, UTF-8, which is what core counts. */
function jsonBytes(value: unknown): number {
  return new TextEncoder().encode(JSON.stringify(value)).length;
}

/** Newest first, cut to the policy and then to the byte budget. */
export function trimmed(history: HistoryVersion[], policy: HistoryPolicy): HistoryVersion[] {
  const kept = policy === "fit" ? [...history] : history.slice(0, Math.max(0, policy));
  while (kept.length > 0 && jsonBytes(kept) > HISTORY_BYTES) kept.pop();
  return kept;
}

/**
 * `next` with `previous` added to its history, when what the entry says
 * changed. Starring it, moving it to another category or attaching a file is
 * no new version.
 *
 * The history starts from `next`'s own, so clearing it sticks, and is trimmed
 * to the policy whenever a version is added.
 */
export function withHistory(
  previous: PasswordEntry | undefined,
  next: PasswordEntry,
  policy: HistoryPolicy,
): PasswordEntry {
  if (!previous || sameContent(previous, next)) return next;
  const version: HistoryVersion = {
    ...(contentOf(previous) as Partial<PasswordEntry>),
    saved_at: previous.updated_at,
  };
  return {
    ...next,
    history: trimmed([version, ...(next.history ?? [])], policy),
  };
}

/** The text fields every entry has. A version leaves out the empty ones, and
 * every client, 1.0.0 on, reads these as strings. */
const BASE_FIELDS = {
  service: "",
  username: "",
  password: "",
  url: "",
  notes: "",
};

/** `entry` with each base text field present. For entries read from the
 * store: one saved by a 1.4 build before restoring kept them is still read. */
export function withBaseFields(entry: PasswordEntry): PasswordEntry {
  const missing = Object.entries(BASE_FIELDS).some(
    ([key]) => typeof (entry as Record<string, unknown>)[key] !== "string",
  );
  if (!missing) return entry;
  const fixed: Record<string, unknown> = { ...entry };
  for (const [key, empty] of Object.entries(BASE_FIELDS)) {
    if (typeof fixed[key] !== "string") fixed[key] = empty;
  }
  return fixed as PasswordEntry;
}

/**
 * `entry` saying what `version` said. A new edit, not a rollback: saved
 * through `withHistory`, the current version goes into the history.
 */
export function restoredFrom(
  entry: PasswordEntry,
  version: HistoryVersion,
  now: number,
): PasswordEntry {
  const kept: Record<string, unknown> = { ...BASE_FIELDS };
  for (const [key, value] of Object.entries(entry)) {
    if (NOT_VERSIONED.has(key)) kept[key] = value;
  }
  const { saved_at: _savedAt, ...content } = version;
  return { ...(kept as PasswordEntry), ...content, updated_at: now };
}

/** `entry` with no history. For a password that leaked, which should not
 * stay in the silo. */
export function withoutHistory(entry: PasswordEntry): PasswordEntry {
  const { history: _history, ...rest } = entry;
  return rest;
}

/** Whether `password` is one this entry had before. */
export function reusesOldPassword(entry: PasswordEntry): boolean {
  const current = entry.password;
  if (!current) return false;
  return (entry.history ?? []).some((v) => v.password === current);
}

/** What a group of fields is called on screen. */
function labelOf(key: string): string {
  if (key === "password") return "Password";
  if (key === "username") return "Username";
  if (key === "service") return "Name";
  if (key === "url") return "Website";
  if (key === "notes") return "Notes";
  if (key === "fields") return "Custom fields";
  if (key === "type") return "Kind";
  if (key.startsWith("totp_")) return "Two-factor code";
  if (key.startsWith("card_")) return "Card";
  if (key.startsWith("id_")) return "Identity";
  if (key.startsWith("ssh_")) return "SSH key";
  return "Other";
}

/** What changed from `older` to `newer`, as the labels a person reads. */
export function changedLabels(
  older: Partial<PasswordEntry>,
  newer: Partial<PasswordEntry>,
): string[] {
  const a = contentOf(older);
  const b = contentOf(newer);
  const labels = new Set<string>();
  for (const key of new Set([...Object.keys(a), ...Object.keys(b)])) {
    if (JSON.stringify(a[key]) !== JSON.stringify(b[key])) labels.add(labelOf(key));
  }
  return [...labels];
}
