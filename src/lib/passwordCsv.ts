import type { PasswordEntry } from "./types";
import { parseTotpInput } from "./totp";
import { appendNotes, noExtras, type ImportExtras } from "./passwordImport";

/** Exporters we recognise by header row. `generic` is the fallback: match
 * whatever columns look right by name, so an unknown tool's export still has
 * a chance of importing rather than being rejected outright. */
export type CsvFormat =
  | "bitwarden"
  | "lastpass"
  | "onepassword"
  | "chrome"
  | "protonpass"
  | "nordpass"
  | "dashlane"
  | "keepass"
  | "roboform"
  | "firefox"
  | "apple"
  | "silentsilo"
  | "generic";

export type ImportResult = {
  format: CsvFormat;
  entries: PasswordEntry[];
  /** Rows recognised but deliberately not imported (e.g. Bitwarden secure
   * notes, which have no password to store in our schema). */
  skipped: number;
  extras: ImportExtras;
};

export class CsvImportError extends Error {}

/**
 * RFC 4180 CSV reader. Hand-rolled rather than pulled in as a dependency
 * because the whole app ships no runtime deps beyond React/lucide, and the
 * grammar is small: quoted fields may contain commas, newlines, and `""`
 * escaped quotes.
 */
export function parseCsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let inQuotes = false;
  let fieldWasQuoted = false;

  // A leading UTF-8 BOM would otherwise become part of the first header
  // name, breaking format detection for exports written by Excel.
  const input = text.replace(/^\uFEFF/, "");

  const endField = () => {
    row.push(fieldWasQuoted ? field : field.trim());
    field = "";
    fieldWasQuoted = false;
  };

  const endRow = () => {
    endField();
    // Skip blank lines rather than emitting a phantom one-empty-field row.
    if (row.length > 1 || row[0] !== "") rows.push(row);
    row = [];
  };

  for (let i = 0; i < input.length; i++) {
    const char = input[i];

    if (inQuotes) {
      if (char === '"') {
        if (input[i + 1] === '"') {
          field += '"';
          i++;
        } else {
          inQuotes = false;
        }
      } else {
        field += char;
      }
      continue;
    }

    if (char === '"') {
      inQuotes = true;
      fieldWasQuoted = true;
    } else if (char === ",") {
      endField();
    } else if (char === "\n") {
      endRow();
    } else if (char === "\r") {
      // CRLF — the \n branch handles the row break.
    } else {
      field += char;
    }
  }

  // A file not ending in a newline still has a final row pending.
  if (field !== "" || row.length > 0) endRow();

  return rows;
}

function normalizeHeader(h: string): string {
  return h.trim().toLowerCase().replace(/^\uFEFF/, "");
}

export function detectFormat(headers: string[]): CsvFormat {
  const set = new Set(headers.map(normalizeHeader));

  // Most distinctive columns first: several exporters share the common
  // name/url/username/password core, so anything checked late must be
  // recognisable only by a column nobody else writes.
  if (set.has("login_password") || set.has("login_uri")) return "bitwarden";
  if (set.has("grouping") && set.has("extra")) return "lastpass";
  if (set.has("vault") && set.has("type")) return "protonpass";
  if (set.has("username2") || set.has("otpsecret")) return "dashlane";
  if (set.has("cardholdername")) return "nordpass";
  if (set.has("rffieldsv2") || set.has("matchurl")) return "roboform";
  if (set.has("group") && set.has("title")) return "keepass";
  if (set.has("httprealm") || set.has("formactionorigin")) return "firefox";
  // Both 1Password and Apple Passwords export an `otpauth` column; only
  // 1Password also writes favourite/archived/tags.
  if (set.has("otpauth")) {
    return set.has("favorite") || set.has("archived") || set.has("tags")
      ? "onepassword"
      : "apple";
  }
  // This app's own export, told apart so its formula guard can come off.
  const normalized = headers.map(normalizeHeader);
  if (
    normalized.length === EXPORT_HEADERS.length &&
    EXPORT_HEADERS.every((h, i) => normalized[i] === h)
  ) {
    return "silentsilo";
  }
  // Chrome's export is exactly name,url,username,password,note (Edge writes
  // the same) — checked after the others because those columns are common
  // enough to collide.
  if (set.has("name") && set.has("url") && set.has("username") && set.has("password")) {
    return "chrome";
  }
  return "generic";
}

/** Column aliases per field, tried in order. Lowercased.
 *
 * `type` is deliberately absent from category: Proton Pass and newer
 * NordPass exports use it for the item kind (login/note/card), and reading
 * it as a category filed every imported entry under "login". */
const FIELD_ALIASES: Record<string, string[]> = {
  service: ["name", "title", "account", "service", "display name"],
  username: ["login_username", "username", "user", "login", "email", "user name"],
  password: ["login_password", "password", "pass", "pwd"],
  url: ["login_uri", "url", "website", "uri", "site", "login_url"],
  notes: ["notes", "note", "extra", "comments"],
  category: ["folder", "grouping", "category", "group", "vault", "tags"],
  totp: ["login_totp", "otpauth", "totp", "otp", "otpsecret", "two-factor secret", "authenticator"],
};

function buildIndex(headers: string[]): Map<string, number> {
  const index = new Map<string, number>();
  headers.forEach((h, i) => {
    const key = normalizeHeader(h);
    // First occurrence wins — some exporters repeat column names.
    if (!index.has(key)) index.set(key, i);
  });
  return index;
}

function pick(row: string[], index: Map<string, number>, field: string): string {
  for (const alias of FIELD_ALIASES[field] ?? []) {
    const at = index.get(alias);
    if (at !== undefined && row[at] !== undefined && row[at] !== "") {
      return row[at];
    }
  }
  return "";
}

/** Returns false when there was a value and it could not be read as TOTP. */
function applyTotp(entry: PasswordEntry, raw: string): boolean {
  if (!raw) return true;
  // Accepts both a bare base32 secret and a full otpauth:// URI, which is
  // what 1Password and (usually) Bitwarden export.
  const parsed = parseTotpInput(raw);
  if (!parsed) return false;
  entry.totp_secret = parsed.secret;
  entry.totp_digits = parsed.digits;
  entry.totp_period = parsed.period;
  entry.totp_algorithm = parsed.algorithm;
  return true;
}

/**
 * Converts CSV text into importable entries. Rows with neither a password
 * nor a TOTP secret are counted as `skipped` rather than imported as empty
 * shells — Bitwarden and 1Password exports interleave secure notes, cards
 * and identities with logins, and none of those fit this schema.
 */
export function csvToEntries(text: string, now: () => number = Date.now): ImportResult {
  const rows = parseCsv(text);
  if (rows.length === 0) throw new CsvImportError("The file is empty.");

  const [headers, ...dataRows] = rows;
  const format = detectFormat(headers);
  const index = buildIndex(headers);

  if (!FIELD_ALIASES.password.some((alias) => index.has(alias))) {
    throw new CsvImportError(
      "No password column found. This does not look like a password export.",
    );
  }

  const entries: PasswordEntry[] = [];
  let skipped = 0;
  const extras = noExtras();
  // Only this app's own file had the guard added, so only there does it
  // come off. The password and TOTP columns are never guarded.
  const unguard = format === "silentsilo" ? removeFormulaGuard : (value: string) => value;

  for (const row of dataRows) {
    const field = (name: string) => unguard(pick(row, index, name));
    const password = pick(row, index, "password");
    const totpRaw = pick(row, index, "totp");
    const service = field("service");

    if (!password && !totpRaw) {
      skipped++;
      continue;
    }

    let url = field("url");
    const noteLines: string[] = [];
    if (format === "bitwarden") {
      // Bitwarden writes every address of a login into one cell, comma
      // separated, and custom fields one per line in `fields`.
      const [first = "", ...more] = url
        .split(",")
        .map((u) => u.trim())
        .filter(Boolean);
      url = first;
      noteLines.push(...more.map((u) => `Web address: ${u}`));
      extras.extraUris += more.length;
      const at = index.get("fields");
      const fields = (at === undefined ? "" : (row[at] ?? ""))
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter(Boolean);
      noteLines.push(...fields);
      extras.customFields += fields.length;
    }

    // A row with secrets but no name is still worth keeping; fall back to
    // the URL or username so it's findable rather than blank in the list.
    const name = service || url || field("username") || "Untitled";

    const timestamp = now();
    const entry: PasswordEntry = {
      id: crypto.randomUUID(),
      service: name,
      username: field("username"),
      password,
      url,
      notes: "",
      category: field("category") || "General",
      created_at: timestamp,
      updated_at: timestamp,
    };
    // Steam and HOTP secrets have no codes here, but they are still the
    // user's second factor.
    if (!applyTotp(entry, totpRaw)) {
      noteLines.push(`Two-factor secret: ${totpRaw}`);
      extras.unsupportedOtp += 1;
    }
    entry.notes = appendNotes(field("notes"), noteLines);
    entries.push(entry);
  }

  if (entries.length === 0) {
    throw new CsvImportError("No importable logins found in that file.");
  }

  return { format, entries, skipped, extras };
}

/**
 * Whether a spreadsheet opening the export would run this cell.
 *
 * `=` always starts a formula. `+`, `-` and `@` are only guarded with a
 * function call, a DDE pipe or a sheet reference behind them, so a phone
 * number such as "+40 721 000 000", a negative amount or an @handle is
 * written as it is: the quote would otherwise travel into whichever manager
 * imports the file. A value already starting with quotes before one of
 * these characters is guarded too, so the importer can take exactly one
 * quote back off.
 */
function needsFormulaGuard(value: string): boolean {
  if (/^[=\t\r]/.test(value)) return true;
  if (/^'+[=+\-@\t\r]/.test(value)) return true;
  if (!/^[+\-@]/.test(value)) return false;
  const rest = value.slice(1);
  return /[|!=]/.test(rest) || /[A-Za-z_.]\s*\(/.test(rest);
}

/** Undoes the guard on this app's own export, including the wider one
 * earlier versions added before every leading =, +, - and @. */
export function removeFormulaGuard(value: string): string {
  return /^'+[=+\-@\t\r]/.test(value) ? value.slice(1) : value;
}

function escapeCsvField(value: string, exact = false): string {
  // Prefixing a quote keeps a name like "=HYPERLINK(...)" inert when the
  // file is opened in Excel. Not for a password or a TOTP secret: another
  // manager importing the file would store the quote as part of it.
  const guarded = !exact && needsFormulaGuard(value) ? `'${value}` : value;
  // Quoted when it starts or ends with a space too: readers, this one
  // included, trim an unquoted field, and a password " pass " came back as
  // "pass".
  return /[",\n\r]|^\s|\s$/.test(guarded) ? `"${guarded.replace(/"/g, '""')}"` : guarded;
}

export const EXPORT_HEADERS = [
  "name",
  "url",
  "username",
  "password",
  "totp",
  "category",
  "note",
] as const;

/**
 * Serialises to the Chrome-style column set, which is the most widely
 * accepted shape across importers (Bitwarden, 1Password and KeePass all
 * read it). Deliberately plaintext: the point of this feature is that a
 * user can always leave with their data, and every other manager's importer
 * expects an unencrypted CSV. The UI is responsible for warning about that.
 */
export function entriesToCsv(entries: PasswordEntry[]): string {
  const lines = [EXPORT_HEADERS.join(",")];

  for (const entry of entries) {
    lines.push(
      [
        escapeCsvField(entry.service ?? ""),
        escapeCsvField(entry.url ?? ""),
        escapeCsvField(entry.username ?? ""),
        escapeCsvField(entry.password ?? "", true),
        escapeCsvField(entry.totp_secret ?? "", true),
        escapeCsvField(entry.category ?? ""),
        escapeCsvField(entry.notes ?? ""),
      ].join(","),
    );
  }

  // Trailing newline: POSIX tools and some importers expect a final line
  // terminator, and its absence has tripped up more than one CSV reader.
  return `${lines.join("\n")}\n`;
}

export function formatLabel(format: CsvFormat): string {
  switch (format) {
    case "bitwarden":
      return "Bitwarden";
    case "lastpass":
      return "LastPass";
    case "onepassword":
      return "1Password";
    case "chrome":
      return "Chrome/Edge";
    case "protonpass":
      return "Proton Pass";
    case "nordpass":
      return "NordPass";
    case "dashlane":
      return "Dashlane";
    case "keepass":
      return "KeePass";
    case "roboform":
      return "RoboForm";
    case "firefox":
      return "Firefox";
    case "apple":
      return "Apple Passwords";
    case "silentsilo":
      return "SilentSilo";
    case "generic":
      return "generic CSV";
  }
}
