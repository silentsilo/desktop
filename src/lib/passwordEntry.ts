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
