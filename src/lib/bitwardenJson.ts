import type { PasswordEntry } from "./types";
import { parseTotpInput } from "./totp";

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
};

export class JsonImportError extends Error {}

type BwItem = {
  type?: number;
  name?: string;
  notes?: string | null;
  folderId?: string | null;
  login?: {
    username?: string | null;
    password?: string | null;
    totp?: string | null;
    uris?: { uri?: string | null }[] | null;
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

  for (const item of parsed.items) {
    const base: PasswordEntry = {
      id: crypto.randomUUID(),
      service: item.name ?? "",
      username: "",
      password: "",
      url: "",
      notes: item.notes ?? "",
      category: (item.folderId && folderNames.get(item.folderId)) || "General",
      created_at: now(),
      updated_at: now(),
    };

    switch (item.type) {
      case 1: {
        const totp = item.login?.totp ? parseTotpInput(item.login.totp) : null;
        entries.push({
          ...base,
          username: item.login?.username ?? "",
          password: item.login?.password ?? "",
          url: item.login?.uris?.[0]?.uri ?? "",
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
        const extras = (
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
          notes: [base.notes, ...extras].filter(Boolean).join("\n"),
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
  }

  return { entries, skipped };
}
