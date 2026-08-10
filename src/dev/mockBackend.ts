/**
 * A stand-in for the Rust side, so the pre-unlock screens can be opened in a
 * plain browser.
 *
 * Those screens — the silo picker, enrolment, unlock, recovery — are the ones
 * whose layout most needs looking at, and the ones hardest to reach: each
 * visit otherwise means provisioning a silo, touching a security key, and
 * tearing it all down again. Anything past unlocking is deliberately not
 * mocked; that needs real data and a real vault.
 *
 * Development only, and only when asked for with `?mock` — the build strips
 * this entirely, so it cannot become a way into a real silo.
 */

type Handler = (args: Record<string, unknown>) => unknown;

const silos = [
  {
    id: "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0",
    name: "Personal",
    path: "C:\\Users\\alex\\Documents\\SilentSilo\\Personal",
    last_opened: Math.floor(Date.now() / 1000) - 3600,
    present: true,
    unlocked: true,
  },
  {
    id: "1a2b3c4d-5e6f-7081-9203-a4b5c6d7e8f9",
    name: "Work — a deliberately long name that has to truncate",
    path: "E:\\Backups\\SilentSilo\\Work with a very long path that will not fit on one line",
    last_opened: Math.floor(Date.now() / 1000) - 86400 * 9,
    present: false,
    unlocked: false,
  },
];

const ROOT_ID = "00000000-0000-0000-0000-0000000000ff";

/// Which entries are starred. Mutable so the context menu's star does
/// something here rather than looking broken.
const starred = new Set<string>(["33333333-3333-3333-3333-333333333333"]);

function folder(id: string, name: string, path: string, parentId: string | null = null) {
  return {
    kind: "folder",
    id,
    parent_id: parentId,
    name,
    path,
    created_at: 1700049600,
    updated_at: 1700049600,
    favorite: starred.has(id),
  };
}

function file(id: string, name: string, size: number) {
  return {
    kind: "file",
    id,
    folder_id: "00000000-0000-0000-0000-0000000000ff",
    name,
    blob_id: id,
    size_bytes: size,
    mime_type: "application/octet-stream",
    content_hash: null,
    created_at: 1700049600,
    updated_at: 1700049600,
    favorite: starred.has(id),
  };
}

function flag(name: string): boolean {
  return new URLSearchParams(location.search).has(name);
}

/** Which screen to render, since they are chosen by backend state. */
function scenario(): string {
  return new URLSearchParams(location.search).get("mock") || "picker";
}

/// Set by `silo_open`, so the `reopen` scenario can model the case the
/// picker cannot show on its own: stepping into a silo that was left
/// unlocked, where no key is asked for and the explorer has to arrive
/// already pointed at a folder.
let steppedInto: string | null = null;

/// Set by `vault_unlock`, because the real backend reports a session as
/// unlocked once it is. Without this the stub answered from the scenario
/// name alone, so any screen that re-read bootstrap after unlocking bounced
/// straight back to the key prompt.
let unlockedAtRuntime = false;

/// When the second target was last filled from the first, so the Copies rows
/// change after a seed the way they do against the real backend.
let seededAt: number | null = null;

/// Set by `silo_create`, so first-run continues into enrolment the way it
/// does against the real backend: a freshly created silo is provisioned,
/// open, and has no key yet, which is the one state that shows EnrollView.
let createdSilo: (typeof silos)[number] | null = null;

/// Set by `fido_enroll_primary`, which is what moves the created silo past
/// the enrolment screen and into the explorer.
let enrolledAtRuntime = false;

const handlers: Record<string, Handler> = {
  silo_open: (args) => {
    steppedInto = String(args.id ?? "");
    return silos.find((s) => s.id === steppedInto) ?? silos[0];
  },
  silo_create: (args) => {
    createdSilo = {
      id: "c0ffee00-0000-4000-8000-000000000001",
      name: String(args.name ?? "Silo"),
      path:
        String(args.location ?? "") ||
        `C:\\Users\\alex\\Documents\\SilentSilo\\${String(args.name ?? "Silo")}`,
      last_opened: Math.floor(Date.now() / 1000),
      present: true,
      unlocked: true,
    };
    enrolledAtRuntime = false;
    return createdSilo;
  },
  fido_enroll_primary: () => {
    enrolledAtRuntime = true;
    return null;
  },
  // Also what backing out of enrolment calls; dropping the created silo
  // returns first-run to the empty picker, same as the real backend.
  silo_forget: () => {
    createdSilo = null;
    enrolledAtRuntime = false;
    return null;
  },
  // "empty" is first-run: no silos at all, which is the state where the
  // picker has no list to fall back to.
  silo_list: () =>
    scenario() === "empty" ? (createdSilo ? [createdSilo] : []) : silos,
  // A silo mid-rotation with a leftover working copy: the shape the report
  // exists for, rather than the one where everything is fine.
  silo_report: () => ({
    app_version: "1.0.0",
    name: "Personal",
    path: String.raw`C:\Users\alex\Documents\SilentSilo\Personal`,
    silo_id: "8f14e45f-ceea-467a-9575-8bd3f1a2c4d5",
    marker_version: 1,
    formats: {
      silo_files: 1,
      marker: 1,
      index_schema: 1,
      sealed_payload: 1,
      blob: 1,
      recovery_envelope: 1,
    },
    files: [
      { name: "vault.db.enc", present: true, bytes: 1_638_400, modified: 1_755_700_000 },
      { name: "vault.db.enc.bak", present: true, bytes: 1_631_000, modified: 1_755_600_000 },
      { name: "vault.db.enc.next", present: false, bytes: null, modified: null },
      { name: "master.dek.enc", present: true, bytes: 96, modified: 1_750_000_000 },
      { name: "content.kek.enc", present: true, bytes: 96, modified: 1_750_000_000 },
      { name: "vault.salt", present: true, bytes: 16, modified: 1_750_000_000 },
      { name: "silo.json", present: true, bytes: 84, modified: 1_750_000_000 },
      { name: "keys/fido.json", present: true, bytes: 1_204, modified: 1_754_000_000 },
      { name: "keys/recovery.json", present: true, bytes: 412, modified: 1_750_000_000 },
    ],
    keys: { total: 3, platform: 1, portable: 2, revoked: 1 },
    recovery_envelope: true,
    working_copy: { name: "vault.db", present: true, bytes: 1_642_496, modified: 1_755_701_000 },
    sync_provider: null,
    disk_free_bytes: 129_000_000_000,
    disk_total_bytes: 639_000_000_000,
  }),
  // Mirrors the Rust matcher closely enough to exercise the warning.
  silo_sync_provider_at: (args) => {
    const path = String(args.path ?? "").toLowerCase();
    for (const [needle, name] of [
      ["onedrive", "OneDrive"],
      ["dropbox", "Dropbox"],
      ["google drive", "Google Drive"],
      ["icloud", "iCloud Drive"],
    ] as const) {
      if (path.includes(needle)) return name;
    }
    return null;
  },
  silo_default_location: (args) =>
    `C:\\Users\\alex\\Documents\\SilentSilo\\${String(args.name ?? "Silo")}`,
  app_bootstrap: () => {
    const which = scenario();
    // A silo created this session: open, keyless until enrolment finishes.
    if (createdSilo) {
      return {
        provisioned: true,
        locked: false,
        fido_available: true,
        fido_key_present: true,
        fido_enrolled: enrolledAtRuntime,
        fido_backup_enrolled: false,
        platform_authenticator: !flag("nohello"),
        portable_enrolled: !flag("helloonly"),
        platform_enrolled: flag("helloonly"),
        silo: createdSilo,
      };
    }
    // No silo open in the picker scenarios — that is what makes the picker
    // the thing on screen.
    // `reopen` starts on the picker and becomes an open, unlocked silo the
    // moment one is stepped into.
    const steppedIn = which === "reopen" && steppedInto !== null;
    const silo =
      (which === "picker" || which === "empty" || which === "reopen") && !steppedIn
        ? null
        : silos.find((s) => s.id === steppedInto) ?? silos[0];
    return {
      provisioned: silo !== null,
      locked:
        which !== "unlocked" && which !== "resume" && !steppedIn && !unlockedAtRuntime,
      fido_available: true,
      fido_key_present: true,
      fido_enrolled:
        which === "unlock" ||
        which === "recovery" ||
        which === "unlocked" ||
        which === "resume" ||
        steppedIn,
      fido_backup_enrolled: false,
      // `?nohello` models a machine where Windows Hello was never set up, which
      // is what `WebAuthNIsUserVerifyingPlatformAuthenticatorAvailable` answers
      // false to. The enrolment screen drops a button in that case, and that
      // branch had no way of being looked at.
      platform_authenticator: !flag("nohello"),
      // `?helloonly` models a silo whose only way in is Windows Hello, which
      // is what changes the unlock screen's instruction: telling that user to
      // insert a key sends them looking for hardware they do not own.
      portable_enrolled: !flag("helloonly"),
      platform_enrolled: flag("helloonly"),
      silo,
    };
  },
  // Configured, because the per-file badges only exist once a backup does.
  // ?archive gives the silo an append-only copy, which is what changes the
  // wording of the delete-for-good dialogs.
  sync_status: () => ({
    configured: true,
    pending_ops: 1,
    archive_targets: flag("archive") ? 1 : 0,
  }),
  // `?mock=unlocked&behind` puts the app in the state a device reaches after
  // being away while the silo was compacted, which is otherwise unreachable
  // in a preview: it needs two devices, a bucket, and a month of clock.
  // The shared Gmail/Bank password reads as breached, so the Health panel's
  // check has something to show without a network.
  pwned_range: (args) =>
    args.prefix === "E891C"
      ? "6C1AF0DBD3E66DB4267118C543E2848818D:31337\n0000000000000000000000000000000000A:0"
      : "0000000000000000000000000000000000A:0",
  sync_now: () => ({
    configured: true,
    ops_pushed: 0,
    ops_fetched: 0,
    ops_applied: 0,
    blobs_uploaded: 0,
    blobs_failed: 0,
    renamed: [],
    needs_rebuild: flag("behind"),
    compacted: 0,
  }),
  // Three imaginary files, so Ctrl+V exercises the import path (and the
  // disk-space check in front of it) without a real clipboard.
  clipboard_file_paths: () => ["C:\\tmp\\a.bin", "C:\\tmp\\b.bin", "C:\\tmp\\c.bin"],
  // ?fullcopy starts with the setting already on, so both states of the
  // toggle can be looked at.
  full_copy_status: () => ({
    enabled: flag("fullcopy"),
    missing: 1,
    missing_bytes: 4_194_304,
  }),
  set_full_copy: () => null,
  vault_rebuild_from_snapshot: () => 12,
  // ?nospace and ?tightspace put the two warnings on screen. The real ones
  // need a genuinely full disk, which is not something to arrange by hand.
  vault_disk_space: (args) => {
    const paths = (args.paths as string[]) ?? [];
    const wanted = paths.length * 250 * 1024 * 1024;
    const headroom = 2 * 1024 * 1024 * 1024;
    const available = flag("nospace") ? 40 * 1024 * 1024 : flag("tightspace") ? headroom : 120 * 1024 * 1024 * 1024;
    const verdict = available < wanted ? "insufficient" : available < wanted + headroom ? "tight" : "fine";
    return {
      available_bytes: available,
      total_bytes: 500 * 1024 * 1024 * 1024,
      verdict,
      wanted_bytes: wanted,
      headroom_bytes: headroom,
    };
  },

  protected_folders_list: () => [
    { path: "C:\\Users\\alex\\Documents\\Taxes", target: "/Taxes" },
    { path: "D:\\Photos\\2026", target: "/2026" },
  ],
  protected_folders_add: () => null,
  protected_folders_remove: () => null,
  protected_folders_scan: () => ({ imported: 3, skipped: 1 }),
  // Slow enough to see the progress count move, since that is the part of
  // fetching everything worth looking at.
  sync_fetch_blob: () => new Promise((resolve) => setTimeout(resolve, 400)),
  // Active, because that state carries the most of this card: a status
  // line, two actions and the footnote under them.
  // `?norecovery` models a silo that has never had a code, which is what puts
  // the setup wizard on screen. Without it that screen only ever appeared on
  // a real first run.
  recovery_status: () =>
    flag("norecovery")
      ? { enabled: false, created_at: null }
      : { enabled: true, created_at: 1_769_000_000 },
  // A different code every call, like the real command, which mints one and
  // overwrites the stored envelope. The old fixed string made a double call
  // look identical to a single one on screen, which is precisely the bug this
  // screen cannot afford to hide.
  recovery_generate: () =>
    Array.from({ length: 8 }, () =>
      Array.from(
        { length: 4 },
        () => "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"[Math.floor(Math.random() * 32)],
      ).join(""),
    ).join("-"),
  // Reached by pressing Unlock, so the real path runs rather than the app
  // being dropped into a state it never enters by itself.
  vault_unlock: () => {
    unlockedAtRuntime = true;
    return { vault_id: silos[0]!.id, created_at: 1700049600, revision: 42 };
  },
  vault_meta: () => ({ vault_id: silos[0]!.id, created_at: 1700049600, revision: 42 }),
  vault_root_folder: () => folder(ROOT_ID, "root", "/"),
  // Named so the shell dialogs, which navigate by id rather than listing
  // from the root each time, can be browsed rather than stalling on the
  // first step into a subfolder.
  vault_get_folder: (args) => {
    const id = String(args.folderId ?? "");
    const named: Record<string, string> = {
      "11111111-1111-1111-1111-111111111111": "Invoices",
      "22222222-2222-2222-2222-222222222222": "Photos",
    };
    const name = named[id];
    return name ? folder(id, name, `/${name}`, ROOT_ID) : folder(ROOT_ID, "root", "/");
  },
  vault_list_folder: (args) =>
    String(args.folderId ?? ROOT_ID) === ROOT_ID
      ? [
          folder("11111111-1111-1111-1111-111111111111", "Invoices", "/Invoices", ROOT_ID),
          folder("22222222-2222-2222-2222-222222222222", "Photos", "/Photos", ROOT_ID),
          file("33333333-3333-3333-3333-333333333333", "Passport scan.pdf", 2_400_000),
          file(
            "44444444-4444-4444-4444-444444444444",
            "A rather long file name that has to truncate somewhere.txt",
            8_192,
          ),
          file("77777777-7777-7777-7777-777777777777", "Archive 2019.zip", 940_000_000),
        ]
      : [],
  vault_list_all_folders: () => [
    folder("11111111-1111-1111-1111-111111111111", "Invoices", "/Invoices", ROOT_ID),
  ],
  vault_list_trash: () => [
    { ...file("55555555-5555-5555-5555-555555555555", "Old contract.pdf", 120_000), original_path: "/Invoices" },
    { ...folder("66666666-6666-6666-6666-666666666666", "Scratch", "/Scratch"), original_path: "/" },
    { ...file("55555555-5555-5555-5555-555555555556", "Screenshot 2019.png", 840_000), original_path: "/Photos" },
    { ...file("55555555-5555-5555-5555-555555555557", "notes.txt", 2_048), original_path: "/" },
  ],
  vault_purge_items: (args) => (args.ids as string[]).length,
  // Answers with hits from more than one folder, since that is the whole
  // point of the results view: the path on each row is what tells the user
  // where a match actually lives. It used to return nothing whatever was
  // typed, so the only search state the preview could show was the empty one.
  vault_search: (args) => {
    const query = String(args.query ?? "").toLowerCase().trim();
    if (!query) return [];
    const pool = [
      { entry: file("33333333-3333-3333-3333-333333333333", "Passport scan.pdf", 2_400_000), folder_path: "/" },
      {
        entry: file(
          "44444444-4444-4444-4444-444444444444",
          "A rather long file name that has to truncate somewhere.txt",
          8_192,
        ),
        folder_path: "/",
      },
      { entry: file("77777777-7777-7777-7777-777777777777", "Archive 2019.zip", 940_000_000), folder_path: "/" },
      { entry: file("88888888-8888-8888-8888-888888888888", "Invoice 2024-03.pdf", 64_000), folder_path: "/Invoices" },
      {
        entry: folder("22222222-2222-2222-2222-222222222222", "Photos", "/Photos", ROOT_ID),
        folder_path: "/",
      },
    ];
    return pool
      .filter((hit) => hit.entry.name.toLowerCase().includes(query))
      .map((hit) => ({ ...hit.entry, folder_path: hit.folder_path }));
  },
  // Four devices covering every way a row can be named: renamed by a
  // person, named only by the machine, and one that has said nothing at all
  // and falls back to a short id.
  // Enough rows to page through, and a search that filters them, so both
  // controls can be exercised without a real log behind them.
  vault_activity: (args) => {
    const q = (args.query ?? {}) as {
      search?: string;
      after?: { lamport: number } | null;
      limit?: number;
    };
    const all = Array.from({ length: 120 }, (_, i) => ({
      op_id: `op-${120 - i}`,
      lamport: 400 - i,
      device_id:
        i % 3 === 0
          ? "d0000000-0000-0000-0000-000000000002"
          : "d0000000-0000-0000-0000-000000000001",
      device_label: i % 3 === 0 ? "Work laptop" : "",
      at: Math.floor(Date.now() / 1000) - i * 3600,
      summary:
        i % 5 === 0 ? `Added report-${120 - i}.pdf` : `Renamed a folder to Invoices ${120 - i}`,
      unknown: i === 9,
    }));

    const needle = (q.search ?? "").trim().toLowerCase();
    const matching = needle
      ? all.filter(
          (e) =>
            e.summary.toLowerCase().includes(needle) ||
            e.device_label.toLowerCase().includes(needle),
        )
      : all;

    const after = q.after;
    const found = after ? matching.findIndex((e) => e.lamport < after.lamport) : 0;
    const start = found < 0 ? matching.length : found;
    const slice = matching.slice(start, start + (q.limit ?? 50));
    const last = slice[slice.length - 1];

    return {
      entries: slice,
      truncated_before: 279,
      next:
        last && start + slice.length < matching.length
          ? { lamport: last.lamport, device_id: last.device_id, op_id: last.op_id }
          : null,
      total: needle ? null : all.length,
    };
  },
  vault_list_devices: () => [
    {
      id: "d0000000-0000-0000-0000-000000000001",
      label: null,
      system_name: "ALEX-DESKTOP",
      platform: "Windows 11 Pro",
      is_this_device: true,
      operations: 428,
      last_change_at: Math.floor(Date.now() / 1000) - 600,
    },
    {
      id: "d0000000-0000-0000-0000-000000000002",
      label: "Work laptop",
      system_name: "SH-LAPTOP-02",
      platform: "Windows 11 Pro",
      is_this_device: false,
      operations: 96,
      last_change_at: Math.floor(Date.now() / 1000) - 86400 * 3,
    },
    {
      id: "d0000000-0000-0000-0000-000000000003",
      label: null,
      system_name: "silo-nas",
      platform: "Debian GNU/Linux 13 (trixie)",
      is_this_device: false,
      operations: 51,
      last_change_at: Math.floor(Date.now() / 1000) - 86400 * 21,
    },
    {
      id: "d0000000-0000-0000-0000-000000000004",
      label: null,
      system_name: null,
      platform: null,
      is_this_device: false,
      operations: 4,
      last_change_at: 0,
    },
  ],
  vault_set_device_label: () => null,
  vault_set_favorite: (args) => {
    const id = String(args.id);
    if (args.favorite) starred.add(id);
    else starred.delete(id);
    return null;
  },
  // Every entry the explorer can show, filtered by what is starred, so the
  // long name and the enormous file are reachable here too: those are the
  // two that break a card layout.
  vault_list_favorites: () =>
    [
      {
        ...folder("11111111-1111-1111-1111-111111111111", "Invoices", "/Invoices", ROOT_ID),
        folder_path: "/",
      },
      {
        ...folder("22222222-2222-2222-2222-222222222222", "Photos", "/Photos", ROOT_ID),
        folder_path: "/",
      },
      {
        ...file("33333333-3333-3333-3333-333333333333", "Passport scan.pdf", 2_400_000),
        folder_path: "/",
      },
      {
        ...file(
          "44444444-4444-4444-4444-444444444444",
          "A rather long file name that has to truncate somewhere.txt",
          8_192,
        ),
        folder_path: "/",
      },
      {
        ...file("77777777-7777-7777-7777-777777777777", "Archive 2019.zip", 940_000_000),
        folder_path: "/",
      },
    ].filter((hit) => starred.has(hit.id)),
  vault_list_local_blob_ids: () => ["33333333-3333-3333-3333-333333333333"],
  // One file in each state, so all three badges are reachable: the passport
  // is here and backed up, the long-named one is here but not yet uploaded,
  // and the third exists only in the backup.
  vault_blob_status: () => ({
    local: [
      "33333333-3333-3333-3333-333333333333",
      "44444444-4444-4444-4444-444444444444",
    ],
    unsynced: ["44444444-4444-4444-4444-444444444444"],
    missing: ["55555555-5555-5555-5555-555555555555"],
    missing_bytes: 4_194_304,
    usage: {
      local_bytes: 2_408_192,
      unsynced_bytes: 8_192,
      blob_count: 2,
      unsynced_count: 1,
    },
  }),
  // `?nobackup` shows the screen a silo starts on. Otherwise a connection is
  // reported, because everything about what this computer holds only exists
  // once there is a backup to hold it against.
  s3_get_config: () => {
    if (flag("nobackup")) return null;
    return { kind: "folder", path: "D:\\Backups\\SilentSilo" };
  },
  // The Copies panel with something worth looking at: one target keeping up
  // and one that has been unplugged for weeks. The second state is the whole
  // reason the panel exists and needs a disk in a drawer to reproduce, so it
  // is the default here rather than behind a flag.
  backup_targets_list: () => {
    const now = Math.floor(Date.now() / 1000);
    // Set by backup_target_seed, so the preview shows what filling a place
    // actually does to the row rather than leaving it stuck on "not written
    // to yet", which is the bug this models.
    const filled = seededAt !== null;
    return [
      {
        id: "11111111-1111-1111-1111-111111111111",
        label: "",
        config: { kind: "folder", path: "D:\\Backups\\SilentSilo" },
        primary: true,
        last_success: now - 90,
        ops_behind: 0,
        retry_in: 0,
        archive: false,
      },
      {
        id: "22222222-2222-2222-2222-222222222222",
        label: "Disc extern, birou",
        config: { kind: "folder", path: "E:\\SiloArchive" },
        primary: false,
        last_success: filled ? seededAt! : now - 47 * 24 * 3600,
        ops_behind: filled ? 0 : 128,
        retry_in: filled ? 0 : 900,
        archive: flag("archive"),
      },
    ];
  },
  backup_target_add: () => null,
  backup_target_seed: () => {
    seededAt = Math.floor(Date.now() / 1000);
    return 412;
  },
  // Versioning without object lock, the case worth showing: it looks like
  // protection and is not, for the one operation that matters.
  backup_target_protection: () => ({ versioning: true, object_lock: false }),
  backup_target_remove: () => null,
  // Two silos open, so the switcher, the picker badge and the Explorer
  // silo chooser are all reachable without a security key.
  silo_open_list: () => silos,
  silo_idle_status: () =>
    silos.map((s, i) => ({ id: s.id, idle_seconds: i * 30, auto_lock_minutes: i === 0 ? 5 : null })),
  silo_set_auto_lock: () => null,
  silo_touch: () => null,
  silo_blur: () => null,
  // A plausible-looking fingerprint, so the confirmation step can be seen
  // without an SSH server to point at.
  sftp_probe_host_key: () => "SHA256:uNiVSAcO9Rr8P5x0hLwq3ZfJmT7kEbN2yGdX1aQvCsI",
  // A JSON array of rows, matching the real command. The old object shape
  // predated the row store and made the panel render nothing at all.
  vault_read_passwords: () =>
    JSON.stringify([
      {
        id: "aaaaaaaa-0000-0000-0000-000000000001",
        service: "GitHub",
        username: "alex@example.com",
        password: "correct-horse-battery",
        url: "https://github.com",
        notes: "Work account.",
        category: "Development",
        created_at: 1700049600000,
        updated_at: 1700049600000,
        totp_secret: "JBSWY3DPEHPK3PXP",
        attachments: [
          {
            blob_id: "bbbbbbbb-0000-0000-0000-000000000001",
            name: "recovery-codes.txt",
            size_bytes: 1024,
          },
        ],
      },
      {
        id: "aaaaaaaa-0000-0000-0000-000000000002",
        service: "Gmail",
        username: "alex@example.com",
        password: "hunter2-but-longer",
        url: "https://mail.google.com",
        notes: "",
        category: "Email",
        created_at: 1700049600000,
        updated_at: 1700049600000,
      },
      {
        id: "aaaaaaaa-0000-0000-0000-000000000003",
        service: "Bank",
        username: "alex",
        // Shares Gmail's password, and it is the shorter one: Health has
        // nothing to group and nothing to call weak unless the fixtures
        // contain the mistakes real vaults contain.
        password: "hunter2-but-longer",
        url: "",
        // Multi-line on purpose: the detail view once flattened newlines
        // onto one truncated line, and nothing in the corpus showed it.
        notes: "Branch phone: 021 000 000\nIBAN: RO49 AAAA 1B31 0075 9384 0000\nAsk for Ioana at the desk.\n\nSafe deposit box: #142, key in the drawer.",
        category: "Banking",
        created_at: 1700049600000,
        updated_at: 1700049600000,
      },
      {
        id: "aaaaaaaa-0000-0000-0000-000000000004",
        type: "card",
        service: "Personal Visa",
        username: "",
        password: "",
        url: "",
        notes: "",
        category: "Banking",
        created_at: 1700049600000,
        updated_at: 1700049600000,
        card_holder: "Alex Stanciu",
        card_brand: "Visa",
        card_number: "4111111111111111",
        card_exp_month: "09",
        card_exp_year: "2027",
        card_code: "123",
      },
      {
        id: "aaaaaaaa-0000-0000-0000-000000000005",
        type: "identity",
        service: "Home address",
        username: "",
        password: "",
        url: "",
        notes: "",
        category: "General",
        created_at: 1700049600000,
        updated_at: 1700049600000,
        id_full_name: "Alex Stanciu",
        id_email: "alex@example.com",
        id_phone: "+40 700 000 000",
        id_address: "Str. Exemplu 1",
        id_city: "București",
        id_country: "România",
      },
      {
        id: "aaaaaaaa-0000-0000-0000-000000000006",
        type: "ssh_key",
        service: "Work laptop key",
        username: "",
        password: "",
        url: "",
        notes: "",
        category: "Development",
        created_at: 1700049600000,
        updated_at: 1700049600000,
        ssh_private_key:
          "-----BEGIN OPENSSH PRIVATE KEY-----\nc2FtcGxlLW5vdC1hLXJlYWwta2V5\n-----END OPENSSH PRIVATE KEY-----",
        ssh_public_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeFakeFakeFake dev@mock",
        ssh_fingerprint: "SHA256:mockmockmockmockmockmockmockmockmockmockmoc",
      },
      {
        id: "aaaaaaaa-0000-0000-0000-000000000007",
        type: "note",
        service: "Wi-Fi at the cabin",
        username: "",
        password: "",
        url: "",
        notes: "SSID: CabinNet\nPassword on the router sticker.",
        category: "General",
        created_at: 1700049600000,
        updated_at: 1700049600000,
      },
    ]),
  ssh_generate_keypair: () => ({
    private_key:
      "-----BEGIN OPENSSH PRIVATE KEY-----\nZ2VuZXJhdGVkLWluLW1vY2s\n-----END OPENSSH PRIVATE KEY-----",
    public_key: "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGeneratedMock dev@mock",
    fingerprint: "SHA256:generatedgeneratedgeneratedgeneratedgenerate",
  }),
  vault_upsert_password: () => null,
  vault_delete_password: () => null,
  copy_secret_to_clipboard: () => null,
  password_attach_file: (args) => ({
    blob_id: crypto.randomUUID(),
    name: String(args.path).split(/[\\/]/).pop() ?? "file",
    size_bytes: 48_128,
  }),
  password_open_attachment: () => null,
  password_delete_attachment: () => null,
  fido_reverify: () => null,
  fido_rename_key: () => null,
  vault_paste_paths: () => ({ imported_files: 1, imported_folders: 1, failed: [] }),
  shell_upload_queue_pending: () =>
    flag("dialogs")
      ? // A folder among the files, which is what the context menu hands
        // over when someone right-clicks one: the case that used to fail.
        ["C:\\Users\\alex\\Desktop\\report.pdf", "C:\\Users\\alex\\Desktop\\upload"]
      : [],
  shell_download_queue_pending: () => (flag("dialogs") ? "C:\\Users\\alex\\Desktop" : null),
  // Two keys, one portable and one built in, so the list, its labels and
  // the rename and remove actions are all reachable.
  // Rotation, with the two costs the panel has to state: a key that stops
  // working, and an append-only copy that cannot be re-sealed.
  vault_rotate_key: () => ({
    resealed: 1284,
    kept: [],
    retired: ["Backup key"],
    recovery_code: "MOCK-CODE-1111-2222-3333-4444-5555-6666",
    unchanged_targets: ["Disc extern, birou"],
  }),
  // ?rotating models a key change that stopped half way, which otherwise
  // needs a crash at exactly the right moment to reproduce.
  vault_rotation_pending: () => flag("rotating"),
  // ?norestore shows the case that matters: a rebuild that does not match,
  // which needs a genuinely stale bucket to reproduce.
  vault_test_restore: () => ({
    matches: !flag("norestore"),
    records: 1284,
    entries: 318,
    differences: flag("norestore")
      ? ["only here: file /Invoices::March.pdf deleted=false fav=0 size=88123 hash=ab mime="]
      : [],
    checked_file: "/notes.txt",
    content_error: null,
  }),
  // ?rotten gives the report something to say. A sound silo and a damaged one
  // are different screens, and the damaged one needs a bucket that has
  // actually decayed to reproduce.
  vault_verify: (args) => [
    {
      id: "t1",
      label: "D:\\Backups\\SilentSilo",
      records_read: 1284,
      blobs_checked: 312,
      bytes_read: args.deep ? 2_411_724_800 : 0,
      missing: 0,
      damaged: [],
      unreferenced: 2,
      failed: null,
    },
    {
      id: "t2",
      label: "Disc extern, birou",
      records_read: 1284,
      blobs_checked: 310,
      bytes_read: args.deep ? 2_400_000_000 : 0,
      missing: flag("rotten") ? 1 : 0,
      damaged: flag("rotten")
        ? ["blobs/9f2c.sslo: content does not match the hash recorded for it"]
        : [],
      unreferenced: 0,
      failed: null,
    },
  ],
  vault_rotate_resume: () => 640,
  cancel_verify: () => null,
  cancel_seed: () => null,
  // `?helloonly` cuts this to the built-in key alone, which is the state a
  // real machine is in until someone enrols a portable one: one key, the
  // Remove action disabled because it is the last, and the warning that
  // everything opening this silo is sealed to this computer. The flag
  // already claimed to model that silo; the key list used to ignore it, so
  // the screen it describes could not be looked at.
  // `?orgsilo` models a silo a company provisioned: one escrow key carrying
  // the badge, whose Remove asks for an organisation key rather than being
  // refused. Reaching that state for real needs a second physical key and a
  // silo created as organisation-administered.
  fido_list_keys: () => [
    ...(flag("orgsilo")
      ? [
          {
            credential_id: "a91f4c7e2b8d40158e6a3c5b7d9f0246",
            public_key: "3059",
            key_slot: 0,
            rp_id: "silentsilo",
            label: "Company escrow",
            wrapped_dek: "ef",
            platform: false,
            policy: "org",
          },
        ]
      : []),
    ...(flag("helloonly")
      ? []
      : [
          {
            credential_id: "05c18a6ff87346cdc386528188695bb7",
            public_key: "3059",
            key_slot: 1,
            rp_id: "silentsilo",
            label: "YubiKey 5C",
            wrapped_dek: "ab",
            platform: false,
          },
        ]),
    {
      credential_id: "7b1d0e4a29c84f6bb0b2b1f3a9d7c012",
      public_key: "3059",
      key_slot: 2,
      rp_id: "silentsilo",
      label: flag("helloonly") ? "Primary" : "",
      wrapped_dek: "cd",
      platform: true,
    },
  ],
  // Opening a browser is the one thing a mock must not really do.
  "plugin:opener|open_url": () => null,

  // Event subscriptions: the app registers several at mount, and a
  // rejection there takes the whole tree down before anything renders.
  "plugin:event|listen": () => nextListenerId++,
  "plugin:event|unlisten": () => null,
};

let nextListenerId = 1;

export function installMockBackend() {
  const w = window as unknown as { __TAURI_INTERNALS__?: unknown };
  w.__TAURI_INTERNALS__ = {
    // `getCurrentWebview()` reads this synchronously at import time, so
    // without it the module throws before any component mounts.
    metadata: {
      currentWebview: { label: "main", windowLabel: "main" },
      currentWindow: { label: "main" },
    },
    invoke: (cmd: string, args: Record<string, unknown> = {}) => {
      const handler = handlers[cmd];
      if (!handler) {
        // Loud rather than silent: a screen quietly rendering an empty state
        // because a command was unmocked is exactly the kind of thing that
        // makes a preview lie about the real layout.
        console.warn(`[mock] no handler for ${cmd}`);
        return Promise.reject(new Error(`[mock] ${cmd} is not mocked`));
      }
      return Promise.resolve(handler(args));
    },
    transformCallback: (cb: unknown) => {
      const id = nextListenerId++;
      (w as unknown as Record<string, unknown>)[`_${id}`] = cb;
      return id;
    },
  };
  console.info(`[mock] backend stubbed — scenario "${scenario()}"`);
}
