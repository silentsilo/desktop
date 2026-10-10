export type VaultMeta = {
  revision: number;
  vault_id: string;
};

/** Selectable idle-timeout durations before the silo auto-locks. */
export const AUTO_LOCK_OPTIONS_MINUTES = [5, 15, 30, 60, 120, 240, 360] as const;

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

/** The operating system the backend runs on, as `std::env::consts::OS` names it. */
export type Os = "windows" | "macos" | "linux";

export type Bootstrap = {
  /** Which platform's words to use. Absent from a backend older than the
   * field, which can only be Windows. */
  os?: Os;
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
  /** `"org"` for a key an organisation provisioned and administers: it cannot
   * be removed, and the recovery code cannot be changed, without another one
   * like it. Empty or absent on every silo somebody set up for themselves. */
  policy?: string;
  credential_id: string;
  public_key: string;
  key_slot: number;
  rp_id: string;
  label: string;
  wrapped_dek: string;
  /** Built-in authenticator (Windows Hello, Touch ID) rather than a
   * removable key. Same strength, but it does not survive the machine. */
  platform: boolean;
  /** Whether this computer can unlock with it, decided by the backend with
   * core's rule. Optional because the mock backend predates the field. */
  usable?: boolean;
};

export type Authenticator = "security-key" | "this-device";

/** Where a silo's backup lives. The shapes differ because the questions do. */
export type StoreKind = "s3" | "folder" | "web-dav" | "sftp" | CloudKind;

/** Storage reached by signing in to an account the user already has. */
export type CloudKind = "onedrive" | "dropbox" | "google-drive";

export const CLOUD_KINDS: CloudKind[] = ["onedrive", "dropbox", "google-drive"];

export function isCloudKind(kind: string): kind is CloudKind {
  return (CLOUD_KINDS as string[]).includes(kind);
}

/** Who a sign-in reached, as `cloud_sign_in` returns it. */
export type CloudAccount = {
  id: string;
  label: string;
  freeBytes: number | null;
  totalBytes: number | null;
};

export type CloudSignIn = {
  id: string;
  account: CloudAccount;
};

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
    }
  | CloudConfigView;

export type CloudConfigView = { kind: CloudKind; account: string; folder: string };

export function isCloudView(view: StoreConfigView): view is CloudConfigView {
  return isCloudKind(view.kind);
}

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
  /** Content no backup holds either, found by asking: nothing to download. */
  absent: string[];
  usage: {
    local_bytes: number;
    unsynced_bytes: number;
    blob_count: number;
    unsynced_count: number;
  };
};

/** Where a file's content currently is, from the user's point of view. */
export type FileSyncState =
  | "local-only"
  | "pending"
  | "backed-up"
  | "remote-only"
  | "uploading"
  | "downloading"
  | "absent";

/**
 * How far filling one copy from another has got (`seed-progress`).
 *
 * Objects and bytes both: the object count stands still for minutes on one
 * large blob, and the byte count alone hides that a thousand tiny records
 * are what is left.
 */
export type SeedProgress = {
  objects_done: number;
  objects_total: number;
  bytes_done: number;
  bytes_total: number;
};

/** Where a running sync pass is (`sync-progress`). Gone once it reports. */
export type SyncProgress = {
  silo_id: string;
  phase:
    | "sending-changes"
    | "uploading"
    | "fetching-changes"
    | "downloading"
    | "importing"
    | "applying"
    | "compacting"
    | "checking";
  done: number;
  total: number;
  /** How much of the file this step moves has moved, and how big it is. Both
   * zero where the step is counted in items instead: a single large upload
   * needs these, because `done` stands still for the whole of it. */
  bytes_done: number;
  bytes_total: number;
  file_id: string | null;
  name: string | null;
  /** The copy an upload is going to, named when there is more than one:
   * each copy gets its own run, so the same count goes by once per copy. */
  target?: string | null;
};

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

/** A silo's activity log, as this computer knows it. */
export type AuditStatus = {
  enabled: boolean;
  /** There is a log to read here, on or off. */
  kept: boolean;
  /** The silo is administered by an organisation, whose log it keeps. */
  org_controlled: boolean;
  /** Kept by an organisation: on for good. */
  organisation: boolean;
  retention_days: number | null;
  /** Records on this computer not yet on every copy. */
  waiting: number;
};

/** One event of the activity log, as core reads it. */
export type AuditEntry = {
  device: string;
  /** The event's name, from core. */
  what: string;
  i: number;
  t: number;
  c: number;
  n?: number;
  o?: string;
  l?: string;
  x?: Record<string, unknown>;
};

export type AuditTrail = {
  device: string;
  events: number;
  missing_events: [number, number][];
  missing_segments: number[];
  broken_segments: number[];
};

export type AuditLog = {
  entries: AuditEntry[];
  devices: AuditTrail[];
  unreadable: number;
  copies_unread: string[];
};

/** One page of the log, read and searched on the Rust side. */
export type AuditPage = AuditLog & {
  /** Entries the search matches, or the whole log's count without one. */
  matched: number;
  total: number;
};

/** What a save was, for the silo's activity log. "imported" is logged once
 * for the whole import instead. */
export type EntryChange =
  | "created"
  | "edited"
  | "restored"
  | "history_cleared"
  | "imported"
  | "arranged";

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
  /** Offered by the desktop's SSH agent, which signs with it after asking. */
  ssh_agent?: boolean;
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
  /** Fields the user named. A hidden one is masked and copied like a
   * password. Absent on every entry written before 1.4. */
  fields?: CustomField[];
  /** Earlier versions, newest first (`lib/entryHistory.ts`). */
  history?: HistoryVersion[];
};

export type CustomField = { name: string; value: string; hidden: boolean };

/**
 * A previous version of an entry. It holds what the entry said, not where it
 * was filed or what was attached to it: attachments are counted only from
 * the entry itself, so a blob referenced from history alone would be swept.
 */
export type HistoryVersion = Partial<Omit<PasswordEntry, "history">> & { saved_at: number };

export type NavMode = "push" | "replace" | "index";

export type BreadcrumbSeg = { label: string; path: string };

export type View =
  | "files"
  | "passwords"
  | "favorites"
  | "health"
  | "settings"
  | "trash"
  | "activity";

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

/** A fill the browser extension asked for, waiting for the person to say
 * yes in this window. Carries no secret: the password is read by the Rust
 * side only after the key check passes, and goes straight to the browser. */
export type BrowserFillPrompt = {
  request_id: string;
  /** The tab's host, with its port when it has one. */
  site: string;
  label: string;
  username: string;
  /** Set when the login was not saved for this site, as a sentence. */
  mismatch: string | null;
};

/** A login the browser extension read from a page on the person's click,
 * waiting to be saved here. Carries the password: the window writes the
 * entry, as for any edit. */
export type BrowserSavePrompt = {
  request_id: string;
  /** The tab's host, with its port when it has one. */
  site: string;
  /** The tab's origin, the new entry's address. */
  url: string;
  /** The name a new login starts with. */
  label: string;
  username: string;
  password: string;
  /** The login saved for this site with this username, offered for update. */
  existing: { id: string; label: string } | null;
};

/** Settings > SSH agent. */
export type SshAgentStatus = {
  /** Windows, Linux and macOS. */
  supported: boolean;
  enabled: boolean;
  running: boolean;
  /** Why it is not listening, when it should be. */
  problem: string | null;
  /** The pipe, or the socket's path, that ssh is pointed at. */
  address: string | null;
};

/** A signature an SSH client asked for, waiting for the person. */
export type SshSignPrompt = {
  request_id: string;
  /** The entry's name. */
  key: string;
  program: string | null;
  parent: string | null;
  /** The server's host key fingerprint, when the client named the server. */
  host: string | null;
  /** The user name a login is for. */
  user: string | null;
  /** "git" for a commit or tag signature. */
  namespace: string | null;
  can_remember: boolean;
  require_reauth: boolean;
};

/** Settings > Browser extension. */
export type BrowserExtensionStatus = {
  /** Windows, Linux and macOS. */
  supported: boolean;
  /** Whether the native host shipped with this build. */
  bundled: boolean;
  enabled: boolean;
  /** Whether the channel is actually open, which "enabled" alone does not say. */
  running: boolean;
  /** Fills that sent a password since the app started, newest first. */
  recent: { site: string; label: string; at: number }[];
};
