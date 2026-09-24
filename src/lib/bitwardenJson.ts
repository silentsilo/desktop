import type { PasswordEntry } from "./types";
import { parseTotpInput } from "./totp";
import { appendNotes, noExtras, type ImportExtras } from "./passwordImport";

/**
 * Bitwarden's unencrypted JSON export, the one format their apps write that
 * actually carries cards, identities and SSH keys. Their CSV holds logins
 * and notes only, so anything richer has to come through here.
 *
 * Item `type` values in the export: 1 login, 2 secure note, 3 card,
 * 4 identity, 5 SSH key.
 */

export type JsonImportResult = {
  entries: PasswordEntry[];
  /** Item types newer than this importer knows. */
  skipped: number;
  extras: ImportExtras;
};

export class JsonImportError extends Error {}

type BwItem = {
  type?: number;
  name?: string;
  notes?: string | null;
  folderId?: string | null;
  /** Custom fields. Type 3 is a link to another field and carries no value. */
  fields?: { name?: string | null; value?: string | null; type?: number }[] | null;
  login?: {
    username?: string | null;
    password?: string | null;
    totp?: string | null;
    uris?: { uri?: string | null }[] | null;
    fido2Credentials?: unknown[] | null;
  };
  card?: {
    cardholderName?: string | null;
    brand?: string | null;
    number?: string | null;
    expMonth?: string | null;
    expYear?: string | null;
    code?: string | null;
  };
  identity?: Record<string, string | null | undefined>;
  sshKey?: {
    privateKey?: string | null;
    publicKey?: string | null;
    keyFingerprint?: string | null;
  };
};

export function looksLikeBitwardenJson(text: string): boolean {
  return text.trimStart().startsWith("{");
}

export function bitwardenJsonToEntries(
  text: string,
  now: () => number = Date.now,
): JsonImportResult {
  let parsed: { encrypted?: boolean; folders?: { id?: string; name?: string }[]; items?: BwItem[] };
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new JsonImportError("That file is not valid JSON.");
  }

  if (parsed.encrypted) {
    throw new JsonImportError(
      "This is a password-protected Bitwarden export. Export again with the unencrypted JSON option, import it, then delete the file.",
    );
  }
  if (!Array.isArray(parsed.items)) {
    throw new JsonImportError("This JSON file does not look like a Bitwarden export.");
  }

  // Folder names become categories, which is the closest idea we have.
  const folderNames = new Map<string, string>();
  for (const folder of parsed.folders ?? []) {
    if (folder.id && folder.name) folderNames.set(folder.id, folder.name);
  }

  const entries: PasswordEntry[] = [];
  let skipped = 0;
  const extras = noExtras();

  for (const item of parsed.items) {
    // Any kind of item can carry custom fields, and a hidden one is often a
    // secret, so they go into notes rather than being dropped.
    const fieldLines = (item.fields ?? [])
      .filter((f) => f.type !== 3 && (f.name || f.value))
      .map((f) => `${f.name ?? ""}: ${f.value ?? ""}`);
    const before = entries.length;
    const base: PasswordEntry = {
      id: crypto.randomUUID(),
      service: item.name ?? "",
      username: "",
      password: "",
      url: "",
      notes: appendNotes(item.notes ?? "", fieldLines),
      category: (item.folderId && folderNames.get(item.folderId)) || "General",
      created_at: now(),
      updated_at: now(),
    };

    switch (item.type) {
      case 1: {
        const rawTotp = item.login?.totp ?? "";
        const totp = rawTotp ? parseTotpInput(rawTotp) : null;
        const [firstUri = "", ...moreUris] = (item.login?.uris ?? [])
          .map((u) => u.uri ?? "")
          .filter(Boolean);
        const lines = moreUris.map((uri) => `Web address: ${uri}`);
        extras.extraUris += moreUris.length;
        // Steam and HOTP secrets have no codes here, but they are still the
        // user's second factor.
        if (rawTotp && !totp) {
          lines.push(`Two-factor secret: ${rawTotp}`);
          extras.unsupportedOtp += 1;
        }
        extras.passkeys += item.login?.fido2Credentials?.length ?? 0;
        entries.push({
          ...base,
          username: item.login?.username ?? "",
          password: item.login?.password ?? "",
          url: firstUri,
          notes: appendNotes(base.notes, lines),
          ...(totp
            ? {
                totp_secret: totp.secret,
                totp_digits: totp.digits === 6 ? undefined : totp.digits,
                totp_period: totp.period === 30 ? undefined : totp.period,
                totp_algorithm: totp.algorithm === "SHA-1" ? undefined : totp.algorithm,
              }
            : {}),
        });
        break;
      }
      case 3:
        entries.push({
          ...base,
          type: "card",
          card_holder: item.card?.cardholderName ?? "",
          card_brand: item.card?.brand ?? "",
          card_number: item.card?.number ?? "",
          card_exp_month: item.card?.expMonth ?? "",
          card_exp_year: item.card?.expYear ?? "",
          card_code: item.card?.code ?? "",
        });
        break;
      case 4: {
        const id = item.identity ?? {};
        const name = [id.title, id.firstName, id.middleName, id.lastName]
          .filter(Boolean)
          .join(" ");
        // Fields our identity shape has no slot for still matter to the
        // person who filled them in; they land in notes instead of vanishing.
        const unslotted = (
          [
            ["SSN", id.ssn],
            ["Passport", id.passportNumber],
            ["Licence", id.licenseNumber],
            ["Username", id.username],
          ] as const
        )
          .filter(([, value]) => Boolean(value))
          .map(([label, value]) => `${label}: ${value}`);
        entries.push({
          ...base,
          type: "identity",
          id_full_name: name,
          id_company: id.company ?? "",
          id_email: id.email ?? "",
          id_phone: id.phone ?? "",
          id_address: [id.address1, id.address2, id.address3].filter(Boolean).join(", "),
          id_city: id.city ?? "",
          id_state: id.state ?? "",
          id_zip: id.postalCode ?? "",
          id_country: id.country ?? "",
          notes: appendNotes(base.notes, unslotted),
        });
        break;
      }
      case 5:
        entries.push({
          ...base,
          type: "ssh_key",
          ssh_private_key: item.sshKey?.privateKey ?? "",
          ssh_public_key: item.sshKey?.publicKey ?? "",
          ssh_fingerprint: item.sshKey?.keyFingerprint ?? "",
        });
        break;
      case 2:
        // A secure note carries only its text, and base already holds it.
        entries.push({ ...base, type: "note" });
        break;
      default:
        // Anything newer than this importer knows.
        skipped += 1;
    }
    if (entries.length > before) extras.customFields += fieldLines.length;
  }

  return { entries, skipped, extras };
}
