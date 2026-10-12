import type { PasswordEntry } from "./types";

/**
 * Applies an edit to a password entry without dropping anything the edit did
 * not mention.
 *
 * An entry is stored as one sealed JSON object and synced whole: saving it
 * replaces the stored copy outright. So a build that does not know about a
 * field, because a newer version added it, must not rewrite an entry without
 * it. The user edits a password on the old laptop, and the TOTP secret they
 * added on the new one is gone, with no error and nothing to undo.
 *
 * Spreading rather than listing fields is what makes that impossible, and
 * this function exists so the rule has a name and a test. Never build a saved
 * entry field by field, and never run one through a schema that strips what
 * it does not recognise.
 */
export function withEdits(
  original: PasswordEntry,
  changes: Partial<PasswordEntry>,
): PasswordEntry {
  return { ...original, ...changes };
}

/**
 * The entry as it goes to the backend.
 *
 * Same rule as above: whatever came out of the store and was not edited goes
 * back in untouched.
 */
export function serializeEntry(entry: PasswordEntry): string {
  return JSON.stringify(entry);
}

/**
 * The list as just read, keeping an entry the window already holds whole
 * where the read brought it without its secrets and it has not changed
 * since. The backend lets a protected entry through for a few minutes after
 * a key touch; a list read after that must not fold the entry up under the
 * person reading it.
 */
export function keepRevealed(fresh: PasswordEntry[], held: PasswordEntry[]): PasswordEntry[] {
  const whole = new Map(held.filter((e) => !e.withheld).map((e) => [e.id, e]));
  return fresh.map((entry) => {
    if (!entry.withheld) return entry;
    const known = whole.get(entry.id);
    return known && known.updated_at === entry.updated_at ? known : entry;
  });
}

/** What filing an entry changed: its category, its star, or both. Only those
 * go to the backend, which applies them to the entry as stored. */
export function arrangedChanges(
  previous: PasswordEntry,
  next: PasswordEntry,
): { category?: string; favorite?: boolean } {
  const changes: { category?: string; favorite?: boolean } = {};
  if (next.category !== previous.category) changes.category = next.category;
  if (Boolean(next.favorite) !== Boolean(previous.favorite)) changes.favorite = Boolean(next.favorite);
  return changes;
}
