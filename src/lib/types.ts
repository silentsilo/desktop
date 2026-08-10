export type VaultMeta = {
  revision: number;
  vault_id: string;
};

/** Selectable idle-timeout durations before the silo auto-locks. */
export const AUTO_LOCK_OPTIONS_MINUTES = [5, 15, 30, 60, 120] as const;

export type FolderEntry = {
  id: string;
  parent_id: string | null;
  name: string;
  path: string;
  created_at: number;
  updated_at: number;
  /** Starred. Travels with the entry rather than living on one machine, so
   * the Favourites list is the same on every device. */
  favorite: boolean;
};

/** A device that has written to this silo's log. Derived from the log, not
 * from a registry: there is nowhere to register. */
export type DeviceInfo = {
  id: string;
  /** What someone typed. Wins over the machine's own name. */
  label: string | null;
  /** What the machine calls itself, refreshed whenever it changes. */
  system_name: string | null;
  /** The system it runs, e.g. "Windows 11 Pro". */
  platform: string | null;
  is_this_device: boolean;
  operations: number;
  /** That device's own clock when it last changed something, or 0 if the
   * log predates this being recorded. */
  last_change_at: number;
};

export type VaultEntry =
  | ({ kind: "folder" } & FolderEntry)
  | ({ kind: "file" } & FileEntry);

/** A trashed entry plus the path of the folder it was trashed out of (its
 * own name/path already appears via the entry itself, so this is the
 * *containing* folder's path — where it used to live). */
export type TrashItem = VaultEntry & { original_path: string };

/** A search result plus the folder it lives in — three files called
 * "scan.pdf" are indistinguishable without the path. */
export type SearchHit = VaultEntry & { folder_path: string };

/** One silo as the picker sees it. */
export type Silo = {
  id: string;
  name: string;
  path: string;
  last_opened: number;
  /** False when the folder isn't reachable — an unplugged drive, say. */
  present: boolean;
  /** Unlocked right now, so opening it costs nothing. */
  unlocked: boolean;
};

export type Bootstrap = {
  provisioned: boolean;
  locked: boolean;
  fido_available: boolean;
  fido_key_present: boolean;
  fido_enrolled: boolean;
  fido_backup_enrolled: boolean;
  /** Whether this machine's built-in authenticator can be enrolled. */
  platform_authenticator: boolean;
  /** Whether any enrolled key is a removable one. The unlock screen words
   * its instruction around what is actually enrolled. */
  portable_enrolled: boolean;
  /** Whether any enrolled key is the machine's built-in authenticator. */
  platform_enrolled: boolean;
  /** The silo currently open. Null means show the picker. */
  silo: Silo | null;
};

export type SecurityKeyInfo = {
  /** How the key's wrapped DEK is unwrapped. Every key this build enrols is
   * `"fido2"`; a silo shared with a Mac or Linux machine may carry kinds
   * enrolled there, which appear in the list but cannot unlock anything
   * here. Optional because the mock backend predates the field. */
  kind?: string;
  credential_id: string;
  public_key: string;
  key_slot: number;
  rp_id: string;
  label: string;
  wrapped_dek: string;
  /** Built-in authenticator (Windows Hello, Touch ID) rather than a
   * removable key. Same strength, but it does not survive the machine. */
  platform: boolean;
};

export type Authenticator = "security-key" | "this-device";

/** Where a silo's backup lives. The shapes differ because the questions do. */
export type StoreKind = "s3" | "folder" | "web-dav" | "sftp";

export type StoreConfigView =
  | {
      kind: "s3";
      endpoint: string;
      region: string;
      bucket: string;
      prefix: string;
      access_key_id: string;
      path_style: boolean;
    }
  | { kind: "folder"; path: string }
  | { kind: "web-dav"; url: string; username: string }
  | {
      kind: "sftp";
      host: string;
      port: number;
      username: string;
      path: string;
      auth_method: string;
      /** Shown back to the user, which is the entire point of a fingerprint. */
      host_fingerprint: string | null;
    };

/**
 * What the explorer needs to label each file, read in one call.
 *
 * `local` and `unsynced` overlap: a blob just written is in both, because
 * it is on this disk and has not reached the backup yet.
 */
export type BlobStatus = {
  local: string[];
  unsynced: string[];
  /** Content the silo has that this computer does not: what a "download
   * everything" pass would fetch. Non-empty on a device that just joined or
   * recovered, since it starts with the index and none of the content. */
  missing: string[];
  missing_bytes: number;
  usage: {
    local_bytes: number;
    unsynced_bytes: number;
    blob_count: number;
    unsynced_count: number;
  };
};

/** Where a file's content currently is, from the user's point of view. */
export type FileSyncState = "local-only" | "pending" | "backed-up" | "remote-only";

export type RecoveryStatus = {
  enabled: boolean;
  created_at: number | null;
};

export type FileEntry = {
  id: string;
  folder_id: string;
  name: string;
  blob_id: string;
  size_bytes: number;
  mime_type: string | null;
  content_hash: string | null;
  created_at: number;
  updated_at: number;
  /** See {@link FolderEntry.favorite}. */
  favorite: boolean;
};

/** One user-defined password category. The list itself is stored as a
 * reserved row in the password store (see PW_CATEGORIES_ROW_ID), so it rides
 * the same per-entry sync as the logins and merges later-edit-wins. */
export type PasswordCategory = {
  name: string;
  color: string;
};

/** One file kept with a password entry. The content is an ordinary
 * encrypted blob; the only reference to it lives here, inside the sealed
 * entry, which is why it never appears in the file explorer. */
export type PasswordAttachment = {
  blob_id: string;
  name: string;
  size_bytes: number;
  /** This attachment's content key, wrapped under the vault key. Opening it
   * needs this: content is encrypted under a key of its own so that rotating
   * the vault key never has to rewrite it. Wrapped, so this is ciphertext
   * here exactly as it is in the entry. */
  blob_key: string;
};

/** What kind of credential an entry is. Absent means `login`, which is what
 * every entry was before the other kinds existed. */
export type CredentialType = "login" | "card" | "identity" | "ssh_key" | "note";

/**
 * One credential, stored as a single sealed JSON object and synced whole.
 *
 * The list below is what *this* build knows. An entry read from the store may
 * carry more, written by a newer version, and saving replaces the stored copy
 * outright: dropping a field here deletes it everywhere, with no error and
 * nothing to undo. So an edit is always a spread over what was loaded (see
 * `withEdits` in `lib/passwordEntry.ts`), never an object built field by
 * field, and never the output of a schema that strips what it does not know.
 */
export type PasswordEntry = {
  id: string;
  /** Display name for every kind: the site for a login, the card's label,
   * the person for an identity, the key's purpose for an SSH key. */
  service: string;
  username: string;
  password: string;
  url: string;
  notes: string;
  category: string;
  created_at: number;
  updated_at: number;
  type?: CredentialType;
  // Card fields. The number is stored as typed and masked on display;
  // number and code are secrets, copied through the secret clipboard.
  card_holder?: string;
  card_number?: string;
  card_brand?: string;
  card_exp_month?: string;
  card_exp_year?: string;
  card_code?: string;
  // Identity fields.
  id_full_name?: string;
  id_company?: string;
  id_email?: string;
  id_phone?: string;
  id_address?: string;
  id_city?: string;
  id_state?: string;
  id_zip?: string;
  id_country?: string;
  // SSH key fields. The private key is a secret like a password.
  ssh_private_key?: string;
  ssh_public_key?: string;
  ssh_fingerprint?: string;
  attachments?: PasswordAttachment[];
  /** Starred, like a file. Absent means no, which is what every entry
   * written before Favourites existed says. */
  favorite?: boolean;
  /** Require a fresh authenticator touch (security key or Windows Hello)
   * before revealing or copying this entry's secrets or opening its files.
   * The silo being unlocked proves who opened it, not who is at the screen
   * now; this asks again for the entries where that difference matters. */
  require_reauth?: boolean;
  /** Base32 TOTP secret (RFC 6238), same trust boundary as `password` and
   * stored in the same encrypted row. The optional fields below only matter
   * if the issuer deviates from the common defaults (6 digits, 30s period,
   * SHA-1). */
  totp_secret?: string;
  totp_digits?: number;
  totp_period?: number;
  totp_algorithm?: "SHA-1" | "SHA-256" | "SHA-512";
};

export type NavMode = "push" | "replace" | "index";

export type BreadcrumbSeg = { label: string; path: string };

export type View = "files" | "passwords" | "favorites" | "health" | "settings" | "trash";

export type SyncStatus = "idle" | "syncing" | "ok" | "offline" | "error";

export type ToastKind = "error" | "success" | "info";

export type Toast = {
  id: string;
  kind: ToastKind;
  message: string;
};

/** How long an open silo has gone unused, against its own limit. */
export type SiloIdleStatus = {
  id: string;
  idle_seconds: number;
  /** `null` follows the app-wide default rather than meaning "never". */
  auto_lock_minutes: number | null;
};

/** One recorded change, as the Activity list shows it. */
export type ActivityEntry = {
  op_id: string;
  /** Position in the order every device agrees on. What the list is sorted by. */
  lamport: number;
  device_id: string;
  device_label: string;
  /** The author's own clock. A label, never a sort key. */
  at: number;
  summary: string;
  /** Written by a newer version than this one. Shown rather than hidden. */
  unknown: boolean;
};

/** Where a page ended, so the next one starts exactly after it. */
export type ActivityCursor = {
  lamport: number;
  device_id: string;
  op_id: string;
};

export type ActivityPage = {
  entries: ActivityEntry[];
  /** Non-zero once compaction has removed the start of the history. */
  truncated_before: number;
  /** Where to continue from, or null at the end of the log. */
  next: ActivityCursor | null;
  /** How many records the log holds, or null while searching, where counting
   * would mean decoding every record in the silo. */
  total: number | null;
};

/** What the silo's disk has left, and how a proposed write measures up. */
export type SpaceReport = {
  /** Null when the disk cannot be asked, which means no warning. */
  available_bytes: number | null;
  total_bytes: number | null;
  /** "fine", "tight", "insufficient", or "unknown". */
  verdict: string;
  wanted_bytes: number;
  headroom_bytes: number;
};
