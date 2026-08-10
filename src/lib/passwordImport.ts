import type { PasswordEntry } from "./types";

/**
 * Duplicate detection for imports.
 *
 * Importing the same export twice used to double every entry, because each
 * parsed row gets a fresh `id` and nothing compared it against what the silo
 * already held. Comparison is on content: an entry whose every meaningful
 * field matches one already stored is the same credential, whatever its id.
 *
 * Matching is exact rather than fuzzy. A row whose password changed since the
 * last export is not a duplicate and is imported, so an import can never lose
 * a newer secret; the user ends up with both versions and decides. This keeps
 * the old rule that an import never overwrites what is already there.
 */

/** Fields that say where an entry lives or when it was touched, not what it
 * holds. `category` and `favorite` are how the user filed the credential,
 * not part of it: an entry moved or starred is still the same login. */
const IGNORED_FIELDS = new Set([
  "id",
  "created_at",
  "updated_at",
  "attachments",
  "category",
  "favorite",
]);

/**
 * A stable string identifying an entry by content. Derived from whatever
 * fields the object actually carries rather than a fixed list, so a field
 * added to `PasswordEntry` later counts towards the comparison instead of
 * being silently ignored (which would make two different entries look equal).
 */
export function entryFingerprint(entry: PasswordEntry): string {
  const parts: string[] = [];
  for (const key of Object.keys(entry).sort()) {
    if (IGNORED_FIELDS.has(key)) continue;
    const value = (entry as Record<string, unknown>)[key];
    // Empty, absent and false all mean "not set", and an exporter that writes
    // an empty column must not read as different from one that omits it.
    if (value === undefined || value === null || value === "" || value === false) continue;
    // An absent type means login, so an entry saved before the other kinds
    // existed fingerprints the same as an import that spells it out.
    if (key === "type" && value === "login") continue;
    parts.push(`${key}=${String(value)}`);
  }
  return parts.join("\u0000");
}

export type DedupeResult = {
  /** The entries worth storing, in the order the file listed them. */
  fresh: PasswordEntry[];
  /** How many were dropped: already in the silo, or repeated in the file. */
  duplicates: number;
};

export function dropDuplicates(
  existing: PasswordEntry[],
  incoming: PasswordEntry[],
): DedupeResult {
  const seen = new Set(existing.map(entryFingerprint));
  const fresh: PasswordEntry[] = [];
  let duplicates = 0;

  for (const entry of incoming) {
    const fingerprint = entryFingerprint(entry);
    // Adding as we go also catches a file that lists the same row twice.
    if (seen.has(fingerprint)) {
      duplicates++;
      continue;
    }
    seen.add(fingerprint);
    fresh.push(entry);
  }

  return { fresh, duplicates };
}

/** Where imported entries get filed: as the file says, or all into one. */
export type ImportCategoryChoice = { kind: "file" } | { kind: "into"; category: string };

/**
 * Applies the user's category choice to parsed entries. Runs before the
 * duplicate check, which is safe because the fingerprint ignores category:
 * refiling an import cannot make an already-stored entry look new.
 */
export function applyImportCategory(
  entries: PasswordEntry[],
  choice: ImportCategoryChoice,
): PasswordEntry[] {
  if (choice.kind === "file") return entries;
  const category = choice.category.trim() || "General";
  return entries.map((entry) => ({ ...entry, category }));
}
