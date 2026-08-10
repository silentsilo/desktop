import { mapPool } from "./pool";
import type { PasswordEntry } from "./types";

/**
 * Breach exposure via the Pwned Passwords range API, under k-anonymity.
 *
 * The password is hashed locally with SHA-1 and only the first five hex
 * characters of the hash are ever sent; the response is a bucket of a few
 * hundred suffixes compared here. Neither the password nor enough of its
 * hash to identify it leaves this computer, and the request carries the
 * padding header so even the response size says nothing.
 *
 * The service is somebody else's. Everything read from it is treated as
 * hostile until parsed: a malformed line is skipped, a failed request marks
 * the check unavailable, and no answer of theirs can change what the vault
 * stores. This module never writes.
 */

/**
 * How many range requests are allowed in flight at once.
 *
 * Small on purpose. The whole check is a courtesy call to a free service,
 * it runs in the background while the user reads the page, and finishing
 * two seconds sooner is worth nothing next to being rate limited.
 */
export const RANGE_CONCURRENCY = 6;

/** One password that appears in known breaches, and how often. */
export type Exposure = {
  /** Every entry sharing this password. */
  entryIds: string[];
  /** How many times the breached corpora contain it. */
  count: number;
};

export type PwnedReport = {
  exposures: Exposure[];
  /** How many distinct passwords were checked. */
  checked: number;
  /** How many range requests failed; those passwords stay unjudged. */
  unavailable: number;
};

export async function sha1Hex(text: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-1", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("").toUpperCase();
}

/**
 * Reads a range response into suffix → count. Lines look like
 * `0018A45C4D1DEF81644B54AB7F969B88D65:3`, but the format is theirs to
 * change: anything that does not parse is dropped, not thrown.
 */
export function parseRange(body: string): Map<string, number> {
  const out = new Map<string, number>();
  for (const line of body.split(/\r?\n/)) {
    const at = line.indexOf(":");
    if (at !== 35) continue;
    const suffix = line.slice(0, at).toUpperCase();
    if (!/^[0-9A-F]{35}$/.test(suffix)) continue;
    const count = Number.parseInt(line.slice(at + 1), 10);
    if (!Number.isFinite(count) || count < 0) continue;
    // Padding entries carry count 0 and mean "not breached".
    if (count === 0) continue;
    out.set(suffix, count);
  }
  return out;
}

/**
 * Checks every login password against the breach corpora. `fetchRange` is
 * injected (the Tauri command in the app, a stub in tests) and is called
 * once per distinct hash prefix, however many entries share it.
 */
export async function checkPasswords(
  entries: PasswordEntry[],
  fetchRange: (prefix: string) => Promise<string>,
): Promise<PwnedReport> {
  // Distinct passwords, remembering which entries carry each.
  const byPassword = new Map<string, string[]>();
  for (const entry of entries) {
    if (!entry.password) continue;
    const ids = byPassword.get(entry.password);
    if (ids) ids.push(entry.id);
    else byPassword.set(entry.password, [entry.id]);
  }

  const hashed = await Promise.all(
    [...byPassword.entries()].map(async ([password, entryIds]) => ({
      hash: await sha1Hex(password),
      entryIds,
    })),
  );

  // One request per distinct prefix, not per password, and a few at a time
  // rather than all at once. A silo with a few hundred logins has a few
  // hundred distinct prefixes, and firing them together asks somebody
  // else's service for all of it in one breath: the answers that come back
  // are refusals, and the report reads as "could not check" rather than as
  // "asked too hard".
  const ranges = new Map<string, Map<string, number> | null>();
  const prefixes = [...new Set(hashed.map((h) => h.hash.slice(0, 5)))];
  await mapPool(prefixes, RANGE_CONCURRENCY, async (prefix) => {
    try {
      ranges.set(prefix, parseRange(await fetchRange(prefix)));
    } catch {
      // Theirs to break; ours to survive. Marked unknown, never fatal.
      ranges.set(prefix, null);
    }
  });

  const exposures: Exposure[] = [];
  let unavailable = 0;
  for (const { hash, entryIds } of hashed) {
    const range = ranges.get(hash.slice(0, 5));
    if (range === null || range === undefined) {
      unavailable++;
      continue;
    }
    const count = range.get(hash.slice(5)) ?? 0;
    if (count > 0) exposures.push({ entryIds, count });
  }

  exposures.sort((a, b) => b.count - a.count);
  return { exposures, checked: hashed.length, unavailable };
}
