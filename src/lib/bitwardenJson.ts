import { t } from "../i18n";
import type { CustomField, PasswordAttachment, PasswordEntry } from "./types";
import { parseTotpInput } from "./totp";
import { appendNotes, noExtras, UNMATCHED_NOTE, type ImportExtras } from "./passwordImport";

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
    throw new JsonImportError(t("pw.json_invalid"));
  }

  if (parsed.encrypted) {
    throw new JsonImportError(t("pw.json_encrypted"));
  }
  if (!Array.isArray(parsed.items)) {
    throw new JsonImportError(t("pw.json_not_bitwarden"));
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
    // Any kind of item can carry custom fields. Type 1 is hidden and stays
    // masked here; type 3 links to another field and carries no value.
    const fields: CustomField[] = (item.fields ?? [])
      .filter((f) => f.type !== 3 && (f.name || f.value))
      .map((f) => ({ name: f.name ?? "", value: f.value ?? "", hidden: f.type === 1 }));
    const base: PasswordEntry = {
      id: crypto.randomUUID(),
      service: item.name ?? "",
      username: "",
      password: "",
      url: "",
      notes: item.notes ?? "",
      ...(fields.length > 0 ? { fields } : {}),
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
  }

  return { entries, skipped, extras };
}

/** One file from a ".zip (With Attachments)" export, already encrypted into
 * the silo, with the folder Bitwarden filed it under. */
export type ZipFile = { folder: string; attachment: PasswordAttachment };

/** The folder Bitwarden's exporter names after an item: the characters
 * Windows forbids in a name become "_", and runs of "_" one. */
export function bitwardenFolderName(name: string): string {
  return name.replace(/[/\\><:"|?*]/g, "_").replace(/__+/g, "_");
}

/**
 * Puts each file of the zip on the entry its folder names. Bitwarden names
 * the folder after the item, and a second item of the same name gets "_1",
 * "_2" in an order the JSON does not give. So a folder is matched only when
 * exactly one item could have made it; the rest go on one note, named by
 * folder, rather than onto the wrong login or nowhere.
 */
export function attachZipFiles(
  entries: PasswordEntry[],
  files: ZipFile[],
  now: () => number = Date.now,
): { entries: PasswordEntry[]; unmatched: number } {
  const byFolder = new Map<string, number[]>();
  entries.forEach((entry, i) => {
    const folder = bitwardenFolderName(entry.service);
    byFolder.set(folder, [...(byFolder.get(folder) ?? []), i]);
  });
  const owner = (folder: string): number | undefined => {
    const named = byFolder.get(folder) ?? [];
    // "Bank_1" may be the second "Bank" as well as an item called that.
    const suffixed = /^(.*)_\d+$/.exec(folder);
    const twins = suffixed ? (byFolder.get(suffixed[1]!) ?? []).length : 0;
    return named.length === 1 && twins < 2 ? named[0] : undefined;
  };

  const out = entries.map((entry) => ({ ...entry }));
  const unmatched: ZipFile[] = [];
  for (const file of files) {
    const i = owner(file.folder);
    if (i === undefined) {
      unmatched.push(file);
      continue;
    }
    out[i] = { ...out[i]!, attachments: [...(out[i]!.attachments ?? []), file.attachment] };
  }
  if (unmatched.length > 0) {
    out.push({
      id: crypto.randomUUID(),
      type: "note",
      service: UNMATCHED_NOTE,
      username: "",
      password: "",
      url: "",
      notes: [
        "Files from the Bitwarden export that could not be matched to one item by name, with the folder each was in:",
        ...unmatched.map((f) => `${f.folder}/${f.attachment.name}`),
      ].join("\n"),
      category: "General",
      created_at: now(),
      updated_at: now(),
      attachments: unmatched.map((f) => f.attachment),
    });
  }
  return { entries: out, unmatched: unmatched.length };
}
