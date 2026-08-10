/**
 * Pure helpers for the passwords view. No React in here: everything is a
 * function of its inputs, which is what lets the panel split into components
 * without each one dragging the others in.
 */

import type { CredentialType, PasswordCategory, PasswordEntry } from "../../lib/types";

/** Absent means login: every entry predating the other kinds is one. */
export function typeOf(entry: PasswordEntry): CredentialType {
  return entry.type ?? "login";
}

export const TYPE_LABELS: Record<CredentialType, { singular: string; plural: string }> = {
  login: { singular: "Login", plural: "Logins" },
  card: { singular: "Card", plural: "Cards" },
  identity: { singular: "Identity", plural: "Identities" },
  ssh_key: { singular: "SSH key", plural: "SSH keys" },
  note: { singular: "Note", plural: "Notes" },
};

export const CREDENTIAL_TYPES: CredentialType[] = [
  "login",
  "card",
  "identity",
  "ssh_key",
  "note",
];

/** Digits only, however the number was typed or imported. */
export function cardDigits(entry: PasswordEntry): string {
  return (entry.card_number ?? "").replace(/\D/g, "");
}

/** `"1234 5678 9012 3456"`, for a revealed number. */
export function groupCardNumber(digits: string): string {
  return digits.replace(/(.{4})/g, "$1 ").trim();
}

/** What the list shows under the entry's name: enough to tell two apart,
 * never a secret. */
export function subtitleFor(entry: PasswordEntry): string {
  switch (typeOf(entry)) {
    case "login":
      return entry.username;
    case "card": {
      const digits = cardDigits(entry);
      return digits.length >= 4 ? `•••• ${digits.slice(-4)}` : entry.card_holder ?? "";
    }
    case "identity":
      return entry.id_email || entry.id_full_name || "";
    case "ssh_key":
      return entry.ssh_fingerprint ?? "";
    case "note":
      // The first line is the note's own summary of itself.
      return entry.notes.split("\n", 1)[0] ?? "";
  }
}

/** Every non-secret field worth matching when the user types in search. */
export function searchTextFor(entry: PasswordEntry): string {
  return [
    entry.service,
    entry.username,
    entry.url,
    entry.notes,
    entry.card_holder,
    entry.card_brand,
    entry.id_full_name,
    entry.id_company,
    entry.id_email,
    entry.id_phone,
    entry.id_city,
    entry.id_country,
    entry.ssh_fingerprint,
    entry.ssh_public_key,
  ]
    .filter(Boolean)
    .join("\n")
    .toLowerCase();
}

/**
 * The reserved row holding the category list, one per silo.
 *
 * A fixed id on purpose: every device writes the list under the same key,
 * so the store's ordinary later-edit-wins merge is what reconciles two
 * devices changing categories apart. The row is invisible to the panel's
 * entry list because it carries `type: "meta:categories"`.
 */
export const CATEGORIES_ROW_ID = "5c1e7a30-9b44-4e1a-a277-630f8d214b91";

export type CategoriesRow = {
  id: string;
  type: "meta:categories";
  categories: PasswordCategory[];
};

export function isCategoriesRow(row: unknown): row is CategoriesRow {
  return (
    typeof row === "object" &&
    row !== null &&
    (row as { type?: unknown }).type === "meta:categories" &&
    Array.isArray((row as { categories?: unknown }).categories)
  );
}

/** The category every entry can fall back to. Undeletable, so "delete a
 * category" always has somewhere to put the entries it orphans. */
export const FALLBACK_CATEGORY = "General";

const DEFAULT_CATEGORIES: PasswordCategory[] = [
  { name: "General", color: "hsl(260, 60%, 55%)" },
  { name: "Social", color: "hsl(330, 65%, 55%)" },
  { name: "Email", color: "hsl(200, 70%, 50%)" },
  { name: "Banking", color: "hsl(145, 60%, 40%)" },
  { name: "Development", color: "hsl(35, 80%, 50%)" },
  { name: "Shopping", color: "hsl(15, 75%, 55%)" },
  { name: "Work", color: "hsl(220, 55%, 50%)" },
  { name: "Entertainment", color: "hsl(290, 55%, 55%)" },
  { name: "Other", color: "hsl(0, 0%, 50%)" },
];

/** A stable colour for a name the list does not define: same name, same
 * hue, on every device, with nothing to store. */
export function hashColor(name: string): string {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.codePointAt(0)!) >>> 0;
  return `hsl(${hash % 360}, 60%, 50%)`;
}

/**
 * The categories the panel works with.
 *
 * The stored list wins when there is one. Before anything was ever stored,
 * the list is derived: the defaults that are actually in use, in their
 * traditional order, plus anything an import brought in that the defaults
 * never named. Categories only carried by entries (a device ahead of this
 * one, an old import) are appended rather than lost.
 */
export function resolveCategories(
  stored: PasswordCategory[] | null,
  entries: PasswordEntry[],
): PasswordCategory[] {
  const inUse = new Set(entries.map((e) => e.category).filter(Boolean));
  const base = stored ?? DEFAULT_CATEGORIES.filter((c) => inUse.has(c.name));

  const known = new Set(base.map((c) => c.name));
  const extras = [...inUse]
    .filter((name) => !known.has(name))
    .sort()
    .map((name) => ({
      name,
      color: DEFAULT_CATEGORIES.find((c) => c.name === name)?.color ?? hashColor(name),
    }));
  return [...base, ...extras];
}

/** Editor choices: the resolved list, with the fallback always available
 * so a fresh silo has at least one option. */
export function categoryChoices(categories: PasswordCategory[]): string[] {
  const names = categories.map((c) => c.name);
  return names.includes(FALLBACK_CATEGORY) ? names : [FALLBACK_CATEGORY, ...names];
}

export type PasswordGenOptions = {
  length: number;
  upper: boolean;
  lower: boolean;
  digits: boolean;
  symbols: boolean;
};

export const DEFAULT_GEN_OPTIONS: PasswordGenOptions = {
  length: 20,
  upper: true,
  lower: true,
  digits: true,
  symbols: true,
};

export function generatePassword(opts: PasswordGenOptions): string {
  const charsets: string[] = [];
  if (opts.upper) charsets.push("ABCDEFGHIJKLMNOPQRSTUVWXYZ");
  if (opts.lower) charsets.push("abcdefghijklmnopqrstuvwxyz");
  if (opts.digits) charsets.push("0123456789");
  if (opts.symbols) charsets.push("!@#$%^&*()_+-=[]{}|;:,.<>?");
  // Never generate from an empty alphabet, even if every toggle is off.
  if (charsets.length === 0) charsets.push("abcdefghijklmnopqrstuvwxyz");

  const all = charsets.join("");
  // Leave room for at least one character from each selected set.
  const length = Math.max(opts.length, charsets.length);
  const arr = new Uint32Array(length);
  crypto.getRandomValues(arr);

  const pw = charsets.map((set, i) => set[arr[i]! % set.length]!);
  for (let i = charsets.length; i < length; i++) {
    pw.push(all[arr[i]! % all.length]!);
  }
  // Shuffle so the guaranteed one-of-each characters aren't always first.
  for (let i = pw.length - 1; i > 0; i--) {
    const j = arr[i]! % (i + 1);
    [pw[i], pw[j]] = [pw[j]!, pw[i]!];
  }
  return pw.join("");
}

export type PasswordStrength = { score: 0 | 1 | 2 | 3 | 4; label: string; color: string };

/** Quick heuristic (length + character variety), not a real entropy
 * estimate — good enough to steer users away from short/simple passwords. */
export function passwordStrength(pw: string): PasswordStrength {
  if (!pw) return { score: 0, label: "", color: "var(--text-dim)" };
  let score = 0;
  if (pw.length >= 8) score++;
  if (pw.length >= 14) score++;
  if (/[a-z]/.test(pw) && /[A-Z]/.test(pw)) score++;
  if (/\d/.test(pw)) score++;
  if (/[^A-Za-z0-9]/.test(pw)) score++;
  const capped = Math.min(score, 4) as 0 | 1 | 2 | 3 | 4;
  const levels: [string, string][] = [
    ["Very weak", "var(--danger-on-dark)"],
    ["Weak", "var(--strength-weak)"],
    ["Fair", "var(--strength-fair)"],
    ["Good", "var(--strength-good)"],
    ["Strong", "var(--success)"],
  ];
  const [label, color] = levels[capped]!;
  return { score: capped, label, color };
}

/**
 * The web address to open for an entry, or null when the field is not one.
 *
 * A bare host gets `https://`. An explicit `http://` or `https://` is kept.
 * Anything else with a scheme is refused rather than opened: entries arrive
 * from CSV and JSON files exported by other apps, so the field is untrusted
 * input, and `file://`, `smb://` and friends are not websites. The Tauri
 * capability scope stops those too, but a rule that lives only in
 * configuration is one refactor away from not being enforced at all.
 */
export function normalizeUrl(url: string): string | null {
  const trimmed = url.trim();
  if (!trimmed) return null;
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  if (/^[a-zA-Z][a-zA-Z\d+\-.]*:/.test(trimmed)) return null;
  return `https://${trimmed}`;
}

/**
 * Rejects hostnames that are literally loopback/private/link-local, so the
 * favicon fetch below can't be used to probe the user's own LAN or local
 * services (e.g. a stored URL of "http://192.168.1.1" or "http://localhost:9200").
 * This is a literal-address check, not DNS-aware — it doesn't stop rebinding
 * a public hostname to a private IP after the fact, but it blocks the
 * straightforward case a crafted "url" field could otherwise reach.
 */
function isPrivateIpv4(h: string): boolean {
  // Decimal, hex and short forms ("2130706433", "0x7f000001", "127.1") do
  // not need handling here: the URL parser normalises every one of them to
  // dotted quads before this sees them.
  const ipv4 = h.match(/^(\d{1,3})\.(\d{1,3})\.\d{1,3}\.\d{1,3}$/);
  if (!ipv4) return false;
  const a = Number(ipv4[1]);
  const b = Number(ipv4[2]);
  if (a === 127 || a === 10 || a === 0) return true;
  if (a === 172 && b >= 16 && b <= 31) return true;
  if (a === 192 && b === 168) return true;
  if (a === 169 && b === 254) return true;
  return false;
}

function isPrivateIpv6(ip: string): boolean {
  if (ip === "::1" || ip === "::") return true;
  const first = ip.split(":")[0] ?? "";
  // fc00::/7, unique local. fe80::/10, link local.
  if (/^f[cd]/.test(first) || /^fe[89ab]/.test(first)) return true;

  // An IPv4-mapped address reaches the same host. The parser rewrites the
  // readable spelling `::ffff:127.0.0.1` into hextets as `::ffff:7f00:1`,
  // so a check that only knows the dotted form sees neither.
  const mapped = ip.match(/^::ffff:(.+)$/);
  if (!mapped) return false;
  const rest = mapped[1]!;
  if (/^\d{1,3}(?:\.\d{1,3}){3}$/.test(rest)) return isPrivateIpv4(rest);
  const hextets = rest.match(/^([0-9a-f]{1,4}):([0-9a-f]{1,4})$/);
  if (!hextets) return false;
  const hi = parseInt(hextets[1]!, 16);
  const lo = parseInt(hextets[2]!, 16);
  return isPrivateIpv4(`${hi >> 8}.${hi & 0xff}.${lo >> 8}.${lo & 0xff}`);
}

function isPublicHostname(hostname: string): boolean {
  const h = hostname.toLowerCase();
  if (h === "localhost" || h.endsWith(".local") || h.endsWith(".internal")) return false;

  // A URL keeps the brackets around an IPv6 literal, so `::1` never arrives
  // bare. Matching it as a plain string let every private v6 address through,
  // and testing the same string for an "fc" or "fd" prefix threw away the
  // icons of real sites whose names start that way.
  if (h.startsWith("[") && h.endsWith("]")) {
    return !isPrivateIpv6(h.slice(1, -1));
  }
  return !isPrivateIpv4(h);
}

export function faviconUrl(url: string): string | null {
  const normalized = normalizeUrl(url);
  if (!normalized) return null;
  try {
    const hostname = new URL(normalized).hostname;
    return isPublicHostname(hostname) ? `https://${hostname}/favicon.ico` : null;
  } catch {
    return null;
  }
}

export function serviceInitials(service: string): string {
  const words = service.trim().split(/\s+/);
  if (words.length >= 2) return (words[0]![0]! + words[1]![0]!).toUpperCase();
  return service.slice(0, 2).toUpperCase();
}

/**
 * Ink that stays readable on a given tile colour.
 *
 * The tile is a category colour: a default, a name hash, or whatever the
 * user picked. White initials on a yellow or lime tile measured under 3:1,
 * so the letters follow the tile rather than the theme.
 */
export function inkOn(background: string): string {
  const hsl = background.match(/hsl\(\s*[\d.]+\s*,\s*[\d.]+%\s*,\s*([\d.]+)%/i);
  const lightness = hsl ? Number(hsl[1]) : 0;
  return lightness > 58 ? "#0a0e1a" : "#fff";
}

/** Colour lookup over the resolved list, falling back to the name hash so
 * an entry whose category was deleted still renders consistently. */
export function makeColorFor(categories: PasswordCategory[]): (name: string) => string {
  const map = new Map(categories.map((c) => [c.name, c.color]));
  return (name: string) => map.get(name) ?? hashColor(name);
}
