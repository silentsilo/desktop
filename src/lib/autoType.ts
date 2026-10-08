import type { PasswordEntry } from "./types";

/** The window auto-type will type into, as the shortcut found it. */
export type AutoTypeTarget = { title: string; program: string };

/** Words a title or a program name is made of, lower case, without the
 * ones every window has. */
function words(text: string): string[] {
  const COMMON = new Set([
    "exe", "app", "the", "and", "for", "www", "com", "net", "org", "login", "log", "sign",
    "in", "on", "to", "of", "a", "client", "desktop", "window", "microsoft", "windows",
  ]);
  return text
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter((w) => w.length >= 2 && !COMMON.has(w));
}

/** A site's name without its scheme, `www.` or top-level domain:
 * "https://www.github.com/login" is "github". */
function siteName(url: string): string {
  const host = url
    .trim()
    .toLowerCase()
    .replace(/^[a-z]+:\/\//, "")
    .split(/[/?#:]/)[0]!
    .replace(/^www\./, "");
  const parts = host.split(".").filter(Boolean);
  return parts.length >= 2 ? parts[parts.length - 2]! : (parts[0] ?? "");
}

/**
 * How well a login matches the window: its name or its site's name found
 * in the title or the program's name. Zero for none. A suggestion only: a
 * program shows whatever title it likes, so nothing is picked on its own.
 */
export function matchScore(entry: PasswordEntry, target: AutoTypeTarget): number {
  const title = target.title.toLowerCase();
  const program = target.program.toLowerCase().replace(/\.exe$/, "");
  const found = new Set([...words(target.title), ...words(program)]);
  let score = 0;
  const name = entry.service.trim().toLowerCase();
  if (name.length >= 3 && (title.includes(name) || program === name)) score += 4;
  const site = siteName(entry.url);
  if (site.length >= 3 && (title.includes(site) || program.includes(site))) score += 3;
  for (const w of words(entry.service)) if (found.has(w)) score += 1;
  return score;
}

/** The logins auto-type can type (a password to type), best match first,
 * then by name. */
export function rankForAutoType(
  entries: PasswordEntry[],
  target: AutoTypeTarget,
): { entry: PasswordEntry; score: number }[] {
  return entries
    .filter((e) => (e.type ?? "login") === "login" && e.password !== "")
    .map((entry) => ({ entry, score: matchScore(entry, target) }))
    .sort((a, b) => b.score - a.score || a.entry.service.localeCompare(b.entry.service));
}
