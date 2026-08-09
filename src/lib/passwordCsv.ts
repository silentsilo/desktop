import type { PasswordEntry } from "./types";
import { parseTotpInput } from "./totp";

/** Exporters we recognise by header row. `generic` is the fallback: match
 * whatever columns look right by name, so an unknown tool's export still has
 * a chance of importing rather than being rejected outright. */
export type CsvFormat = "bitwarden" | "lastpass" | "onepassword" | "chrome" | "generic";

export type ImportResult = {
  format: CsvFormat;
  entries: PasswordEntry[];
  /** Rows recognised but deliberately not imported (e.g. Bitwarden secure
   * notes, which have no password to store in our schema). */
  skipped: number;
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

  if (set.has("login_password") || set.has("login_uri")) return "bitwarden";
  if (set.has("grouping") && set.has("extra")) return "lastpass";
  if (set.has("otpauth")) return "onepassword";
  // Chrome's export is exactly name,url,username,password,note — checked
  // after the others because those columns are common enough to collide.
  if (set.has("name") && set.has("url") && set.has("username") && set.has("password")) {
    return "chrome";
  }
  return "generic";
}

/** Column aliases per field, tried in order. Lowercased. */
const FIELD_ALIASES: Record<string, string[]> = {
  service: ["name", "title", "account", "service", "display name"],
  username: ["login_username", "username", "user", "login", "email", "user name"],
  password: ["login_password", "password", "pass"],
  url: ["login_uri", "url", "website", "uri", "site", "login_url"],
  notes: ["notes", "note", "extra", "comments"],
  category: ["folder", "grouping", "category", "group", "tags", "type"],
  totp: ["login_totp", "otpauth", "totp", "otp", "two-factor secret", "authenticator"],
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

function applyTotp(entry: PasswordEntry, raw: string): void {
  if (!raw) return;
  // Accepts both a bare base32 secret and a full otpauth:// URI, which is
  // what 1Password and (usually) Bitwarden export.
  const parsed = parseTotpInput(raw);
  if (!parsed) return;
  entry.totp_secret = parsed.secret;
  entry.totp_digits = parsed.digits;
  entry.totp_period = parsed.period;
  entry.totp_algorithm = parsed.algorithm;
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

  if (index.get("password") === undefined && index.get("login_password") === undefined) {
    throw new CsvImportError(
      "No password column found. This doesn't look like a password export.",
    );
  }

  const entries: PasswordEntry[] = [];
  let skipped = 0;

  for (const row of dataRows) {
    const password = pick(row, index, "password");
    const totpRaw = pick(row, index, "totp");
    const service = pick(row, index, "service");

    if (!password && !totpRaw) {
      skipped++;
      continue;
    }
    // A row with secrets but no name is still worth keeping; fall back to
    // the URL or username so it's findable rather than blank in the list.
    const name = service || pick(row, index, "url") || pick(row, index, "username") || "Untitled";

    const timestamp = now();
    const entry: PasswordEntry = {
      id: crypto.randomUUID(),
      service: name,
      username: pick(row, index, "username"),
      password,
      url: pick(row, index, "url"),
      notes: pick(row, index, "notes"),
      category: pick(row, index, "category") || "General",
      created_at: timestamp,
      updated_at: timestamp,
    };
    applyTotp(entry, totpRaw);
    entries.push(entry);
  }

  if (entries.length === 0) {
    throw new CsvImportError("No importable logins found in that file.");
  }

  return { format, entries, skipped };
}

function escapeCsvField(value: string): string {
  // Leading =/+/-/@ make spreadsheet apps evaluate the cell as a formula;
  // prefixing a quote keeps an exported password like "=2+2" readable as
  // text instead of silently becoming 4 when reopened in Excel.
  const guarded = /^[=+\-@]/.test(value) ? `'${value}` : value;
  return /[",\n\r]/.test(guarded) ? `"${guarded.replace(/"/g, '""')}"` : guarded;
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
        entry.service,
        entry.url,
        entry.username,
        entry.password,
        entry.totp_secret ?? "",
        entry.category,
        entry.notes,
      ]
        .map((field) => escapeCsvField(field ?? ""))
        .join(","),
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
      return "Chrome";
    case "generic":
      return "generic CSV";
  }
}
