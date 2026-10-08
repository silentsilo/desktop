/**
 * Pure helpers for the passwords view. No React in here: everything is a
 * function of its inputs, which is what lets the panel split into components
 * without each one dragging the others in.
 */

import type { CredentialType, PasswordCategory, PasswordEntry } from "../../lib/types";
import { t, type Key } from "../../i18n";

/** Absent means login: every entry predating the other kinds is one. */
export function typeOf(entry: PasswordEntry): CredentialType {
  return entry.type ?? "login";
}

type TypeLabel = { readonly singular: string; readonly plural: string };

/** Read when shown, so a language change is picked up. */
function typeLabel(singular: Key, plural: Key): TypeLabel {
  return {
    get singular() {
      return t(singular);
    },
    get plural() {
      return t(plural);
    },
  };
}

export const TYPE_LABELS: Record<CredentialType, TypeLabel> = {
  login: typeLabel("pw.type_login", "pw.type_logins"),
  card: typeLabel("pw.type_card", "pw.type_cards"),
  identity: typeLabel("pw.type_identity", "pw.type_identities"),
  ssh_key: typeLabel("pw.type_ssh_key", "pw.type_ssh_keys"),
  note: typeLabel("pw.type_note", "pw.type_notes"),
};

export const CREDENTIAL_TYPES: CredentialType[] = [
  "login",
  "card",
  "identity",
  "ssh_key",
  "note",
];

/** Whole sentences per kind, because the kind's word changes form in them. */
export const TYPE_TEXTS: Record<
  CredentialType,
  { add: Key; edit: Key; noneYet: Key; count: Key }
> = {
  login: {
    add: "pw.add_login",
    edit: "pw.edit_login",
    noneYet: "pw.none_yet_login",
    count: "pw.count_login",
  },
  card: { add: "pw.add_card", edit: "pw.edit_card", noneYet: "pw.none_yet_card", count: "pw.count_card" },
  identity: {
    add: "pw.add_identity",
    edit: "pw.edit_identity",
    noneYet: "pw.none_yet_identity",
    count: "pw.count_identity",
  },
  ssh_key: {
    add: "pw.add_ssh_key",
    edit: "pw.edit_ssh_key",
    noneYet: "pw.none_yet_ssh_key",
    count: "pw.count_ssh_key",
  },
  note: { add: "pw.add_note", edit: "pw.edit_note", noneYet: "pw.none_yet_note", count: "pw.count_note" },
};

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
      // The first line is the note's own summary of itself, unless the note
      // is protected: then it is the secret, and the list, Favourites and
      // Health would show it without the key touch the detail pane asks for.
      return notesAreSecret(entry) ? t("pw.protected_note") : (entry.notes.split("\n", 1)[0] ?? "");
  }
}

/** Every non-secret field worth matching when the user types in search. */
export function searchTextFor(entry: PasswordEntry): string {
  return [
    entry.service,
    entry.username,
    entry.url,
    // A protected entry's notes are a secret, and matching on them would let
    // search answer questions about text the user has not unlocked.
    notesAreSecret(entry) ? "" : entry.notes,
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
    // A hidden field is found by its name, never by its value.
    ...(entry.fields ?? []).flatMap((f) => (f.hidden ? [f.name] : [f.name, f.value])),
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

/** Tile colours: each carries white initials at 4.9:1 or better. The same
 * list is used on mobile. */
export const AVATAR_PALETTE = [
  "#7c3aed",
  "#4f46e5",
  "#2563eb",
  "#0369a1",
  "#0f766e",
  "#047857",
  "#4d7c0f",
  "#a16207",
  "#b45309",
  "#c2410c",
  "#b91c1c",
  "#be185d",
  "#a21caf",
  "#9333ea",
  "#475569",
] as const;

const DEFAULT_CATEGORIES: PasswordCategory[] = [
  { name: "General", color: "#7c3aed" },
  { name: "Social", color: "#be185d" },
  { name: "Email", color: "#0369a1" },
  { name: "Banking", color: "#047857" },
  { name: "Development", color: "#b45309" },
  { name: "Shopping", color: "#c2410c" },
  { name: "Work", color: "#2563eb" },
  { name: "Entertainment", color: "#a21caf" },
  { name: "Other", color: "#475569" },
];

/** A stable colour for a name the list does not define: same name, same
 * colour, on every device, with nothing to store. */
export function hashColor(name: string): string {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.codePointAt(0)!) >>> 0;
  return AVATAR_PALETTE[hash % AVATAR_PALETTE.length]!;
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

  // Two draws, not one. The shuffle used to index the same array the
  // characters were picked from, so the permutation was a function of the
  // choices rather than independent of them: not a break, but a generator
  // should not be quietly correlating the two halves of its own output.
  const picks = new Uint32Array(length);
  const swaps = new Uint32Array(length);
  crypto.getRandomValues(picks);
  crypto.getRandomValues(swaps);

  const pw = charsets.map((set, i) => set[picks[i]! % set.length]!);
  for (let i = charsets.length; i < length; i++) {
    pw.push(all[picks[i]! % all.length]!);
  }
  // Shuffle so the guaranteed one-of-each characters aren't always first.
  for (let i = pw.length - 1; i > 0; i--) {
    const j = swaps[i]! % (i + 1);
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
    [t("pw.strength_very_weak"), "var(--danger-on-dark)"],
    [t("pw.strength_weak"), "var(--strength-weak)"],
    [t("pw.strength_fair"), "var(--strength-fair)"],
    [t("pw.strength_good"), "var(--strength-good)"],
    [t("pw.strength_strong"), "var(--success)"],
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

type Rgb = [number, number, number];

/** Reads `#rgb`, `#rrggbb`, `rgb()` and `hsl()`; null for anything else. */
export function parseColor(value: string): Rgb | null {
  const v = value.trim();
  const short = v.match(/^#([0-9a-f])([0-9a-f])([0-9a-f])$/i);
  if (short) return [1, 2, 3].map((i) => parseInt(short[i]! + short[i]!, 16)) as Rgb;
  const long = v.match(/^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i);
  if (long) return [1, 2, 3].map((i) => parseInt(long[i]!, 16)) as Rgb;
  const rgb = v.match(/^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i);
  if (rgb) return [Number(rgb[1]), Number(rgb[2]), Number(rgb[3])];
  const hsl = v.match(/^hsla?\(\s*([\d.]+)(?:deg)?[\s,]+([\d.]+)%[\s,]+([\d.]+)%/i);
  if (hsl) {
    const h = Number(hsl[1]) % 360;
    const s = Number(hsl[2]) / 100;
    const l = Number(hsl[3]) / 100;
    const a = s * Math.min(l, 1 - l);
    const f = (n: number) => {
      const k = (n + h / 30) % 12;
      return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))));
    };
    return [f(0), f(8), f(4)];
  }
  return null;
}

/** WCAG relative luminance. */
function luminance([r, g, b]: Rgb): number {
  const f = (c: number) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

function contrast(a: Rgb, b: Rgb): number {
  const la = luminance(a);
  const lb = luminance(b);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

const WHITE: Rgb = [255, 255, 255];
const INK_DARK: Rgb = [10, 14, 26];

function hueOf([r, g, b]: Rgb): { hue: number; sat: number } {
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const d = max - min;
  if (d === 0) return { hue: 0, sat: 0 };
  let hue: number;
  if (max === r) hue = ((g - b) / d) % 6;
  else if (max === g) hue = (b - r) / d + 2;
  else hue = (r - g) / d + 4;
  return { hue: (hue * 60 + 360) % 360, sat: d / max };
}

/**
 * The tile colour for a category colour.
 *
 * Stored categories keep whatever colour they were saved with, and older
 * ones are light hues that white initials cannot sit on. Those are drawn
 * with the palette colour nearest in hue, so the tile still matches the
 * rail dot and the initials still read.
 */
export function avatarColor(color: string): string {
  const rgb = parseColor(color);
  if (!rgb) return AVATAR_PALETTE[0];
  if (contrast(rgb, WHITE) >= 4.5) return color;
  const { hue, sat } = hueOf(rgb);
  if (sat < 0.15) return "#475569";
  let best: string = AVATAR_PALETTE[0];
  let bestDistance = 360;
  for (const candidate of AVATAR_PALETTE) {
    const c = hueOf(parseColor(candidate)!);
    if (c.sat < 0.15) continue;
    const distance = Math.min(Math.abs(c.hue - hue), 360 - Math.abs(c.hue - hue));
    if (distance < bestDistance) {
      bestDistance = distance;
      best = candidate;
    }
  }
  return best;
}

/**
 * Ink that stays readable on a given tile colour, chosen by WCAG luminance:
 * whichever of white and near-black has more contrast with the tile.
 */
export function inkOn(background: string): string {
  const rgb = parseColor(background);
  if (!rgb) return "#fff";
  return contrast(rgb, WHITE) >= contrast(rgb, INK_DARK) ? "#fff" : "#0a0e1a";
}

/** Colour lookup over the resolved list, falling back to the name hash so
 * an entry whose category was deleted still renders consistently. */
export function makeColorFor(categories: PasswordCategory[]): (name: string) => string {
  const map = new Map(categories.map((c) => [c.name, c.color]));
  return (name: string) => map.get(name) ?? hashColor(name);
}

// ── What counts as a secret, and when a touch is asked for ──────────

/**
 * Whether an entry's own notes are one of its secrets.
 *
 * Follows the entry rather than a global rule, because the field is used
 * for both things: a line of context under a login, and the place a
 * recovery code actually ended up. `require_reauth` is the user saying
 * which of the two this entry is, so ticking it covers the notes as well as
 * the password. An entry that did not tick it behaves exactly as before,
 * which is the point: the app should ask for a key touch only where it was
 * asked to.
 */
export function notesAreSecret(entry: PasswordEntry): boolean {
  return entry.require_reauth === true;
}

/** How the list's one-click copy should treat this kind of entry. */
export type CopyKind =
  /** Through the clearing clipboard, behind the entry's re-auth gate. */
  | "secret"
  /** The ordinary clipboard: this is the part meant to be handed out. */
  | "plain";

/**
 * What the one-click copy puts on the clipboard, and by which route.
 *
 * A note goes the secret way. It is free text the user chose to store in a
 * password manager, which makes "probably not a secret" the wrong default,
 * and on Windows the ordinary clipboard writes what it holds into Clipboard
 * History on disk and syncs it to the user's other machines. An email
 * address and an SSH public key are the parts of those entries that exist
 * to be given to somebody, so they take the plain route.
 */
export function copyKindFor(entry: PasswordEntry): CopyKind {
  switch (typeOf(entry)) {
    case "login":
    case "card":
    case "note":
      return "secret";
    case "identity":
    case "ssh_key":
      return "plain";
  }
}

/**
 * Whether writing these entries out as a plaintext CSV has to ask first.
 *
 * An export is the broadest reveal the app performs, and it was the one
 * place the "ask again before revealing" flag did not reach: it covered
 * copying, editing and opening an attachment, then the export handed the
 * same secrets over untouched. One touch for the batch, because the user is
 * performing one act.
 */
export function exportNeedsTouch(entries: PasswordEntry[]): boolean {
  return entries.some((entry) => entry.require_reauth === true);
}

/**
 * What the list's one-click copy hands over for this kind of entry.
 *
 * Separate from [`copyKindFor`] so the two questions, what to copy and by
 * which route, stay answerable on their own and testable together.
 */
/** What the one-click copy hands over, named for the activity log. */
export function oneClickField(entry: PasswordEntry): string {
  switch (typeOf(entry)) {
    case "card":
      return "card number";
    case "note":
      return "notes";
    case "identity":
      return "email";
    case "ssh_key":
      return "public key";
    default:
      return "password";
  }
}

export function oneClickCopyValue(entry: PasswordEntry): string {
  switch (typeOf(entry)) {
    case "login":
      return entry.password;
    case "card":
      return cardDigits(entry);
    case "note":
      return entry.notes;
    case "identity":
      return entry.id_email || entry.id_full_name || "";
    case "ssh_key":
      return entry.ssh_public_key ?? "";
  }
}
