import { useState, useMemo, useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { Download, KeyRound, MousePointerClick, ShieldCheck, Upload } from "lucide-react";
import { ViewHeader } from "../../components/ViewHeader";
import type {
  CredentialType,
  PasswordAttachment,
  PasswordCategory,
  PasswordEntry,
} from "../../lib/types";
import { csvToEntries, entriesToCsv, formatLabel } from "../../lib/passwordCsv";
import { bitwardenJsonToEntries, looksLikeBitwardenJson } from "../../lib/bitwardenJson";
import {
  applyImportCategory,
  dropDuplicates,
  type ImportCategoryChoice,
} from "../../lib/passwordImport";
import { withEdits } from "../../lib/passwordEntry";
import { formatAppError } from "../../lib/errors";
import { runsOnOpen } from "../../lib/executable";
import { ConfirmDialog } from "../ConfirmDialog";
import { EntryList } from "./EntryList";
import { ImportFilingDialog } from "./ImportFilingDialog";
import { EntryDetail } from "./EntryDetail";
import { EntryEditor } from "./EntryEditor";
import { CategoryRail, TYPE_ICONS } from "./CategoryRail";
import {
  copyKindFor,
  CREDENTIAL_TYPES,
  DEFAULT_GEN_OPTIONS,
  exportNeedsTouch,
  FALLBACK_CATEGORY,
  generatePassword,
  makeColorFor,
  oneClickCopyValue,
  resolveCategories,
  searchTextFor,
  TYPE_LABELS,
  typeOf,
} from "./util";
import { IconEye, IconEyeOff, IconPlus, IconSearch } from "../../ui/Icons";

type Props = {
  entries: PasswordEntry[];
  /** The stored category list, or null when this silo never saved one. */
  storedCategories: PasswordCategory[] | null;
  busy: boolean;
  /** An entry another view is sending the user to. Selecting it clears the
   * filters, or the panel would land on an entry the current category or
   * search hides, and show nothing. */
  focusEntryId?: string | null;
  /** Creates or replaces one entry. */
  onSaveEntry: (entry: PasswordEntry) => void;
  onDeleteEntry: (id: string) => void;
  onImportEntries: (entries: PasswordEntry[]) => void;
  /** Replaces the category list as a whole. */
  onSaveCategories: (categories: PasswordCategory[]) => void;
  /** Asks the user to confirm something that runs when opened. */
  onConfirmRun: (name: string) => Promise<boolean>;
};

const SHOW_FAVICONS_KEY = "silentsilo.passwords.showFavicons";

function emptyEntry(type: CredentialType): PasswordEntry {
  return {
    id: crypto.randomUUID(),
    service: "",
    username: "",
    // A login starts with a generated password because that is the one it
    // should end up with; the other kinds carry theirs from elsewhere.
    password: type === "login" ? generatePassword(DEFAULT_GEN_OPTIONS) : "",
    url: "",
    notes: "",
    category: FALLBACK_CATEGORY,
    created_at: Date.now(),
    updated_at: Date.now(),
    ...(type === "login" ? {} : { type }),
  };
}

/**
 * The passwords view: categories on the left, the list in the middle, one
 * entry on the right.
 *
 * Three panes because the three questions are different: "which kind",
 * "which one", "what's in it". The old single column answered all three with
 * cards, which meant four logins per screen and a modal for everything else.
 */
export function PasswordsPanel({
  entries,
  storedCategories,
  busy,
  focusEntryId,
  onSaveEntry,
  onDeleteEntry,
  onImportEntries,
  onSaveCategories,
  onConfirmRun,
}: Props) {
  const [search, setSearch] = useState("");
  const [selectedCategory, setSelectedCategory] = useState<string | null>(null);
  const [selectedType, setSelectedType] = useState<CredentialType | null>(null);
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ entry: PasswordEntry; creating: boolean } | null>(null);
  const [transferBusy, setTransferBusy] = useState(false);
  const [transferNotice, setTransferNotice] = useState<string | null>(null);
  const [transferError, setTransferError] = useState<string | null>(null);
  const [confirmingExport, setConfirmingExport] = useState(false);
  /** Parsed and counted, waiting for the user to say where it gets filed. */
  const [pendingImport, setPendingImport] = useState<{
    imported: PasswordEntry[];
    skipped: number;
    source: string;
    unit: [string, string];
    skippedUnit: [string, string];
  } | null>(null);
  /// Whether the "finish editing first" notice is up. Clicking another row
  /// mid-edit does nothing on purpose, and doing nothing silently read as
  /// the list being broken.
  const [editLockNoticeUp, setEditLockNoticeUp] = useState(false);
  const editLockTimer = useRef<number | null>(null);
  const flashEditLock = useCallback(() => {
    setEditLockNoticeUp(true);
    if (editLockTimer.current !== null) window.clearTimeout(editLockTimer.current);
    editLockTimer.current = window.setTimeout(() => setEditLockNoticeUp(false), 3000);
  }, []);

  // Single shared clock for every visible TOTP code, instead of one timer
  // per entry. Only while there is a code to count down: it re-renders the
  // whole panel once a second, and most entries have no second factor at
  // all, so an unconditional timer spent that on nothing.
  const [now, setNow] = useState(() => Date.now());
  const [countingDown, setCountingDown] = useState(false);
  useEffect(() => {
    if (!countingDown) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [countingDown]);

  // Off by default: fetching a favicon discloses the viewing user's IP and
  // the current time to whatever host is named in a saved entry's URL, for
  // every entry, just by opening this panel. Require explicit opt-in.
  const [showFavicons, setShowFavicons] = useState(
    () => localStorage.getItem(SHOW_FAVICONS_KEY) === "true"
  );

  const toggleShowFavicons = useCallback(() => {
    setShowFavicons((prev) => {
      const next = !prev;
      localStorage.setItem(SHOW_FAVICONS_KEY, String(next));
      return next;
    });
  }, []);

  /// Arriving from Health: show that entry and nothing standing in front of
  /// it. Runs on the id rather than on every render, so a user who then
  /// filters or searches is not dragged back to it.
  useEffect(() => {
    if (!focusEntryId) return;
    setSelectedCategory(null);
    setSelectedType(null);
    setSearch("");
    setSelectedId(focusEntryId);
  }, [focusEntryId]);

  const categories = useMemo(
    () => resolveCategories(storedCategories, entries),
    [storedCategories, entries]
  );
  const colorFor = useMemo(() => makeColorFor(categories), [categories]);

  const categoryCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const e of entries) {
      counts.set(e.category, (counts.get(e.category) ?? 0) + 1);
    }
    return counts;
  }, [entries]);

  /// Renaming a category is renaming it on every entry that carries it:
  /// the entries are the ground truth the counts come from, and a list
  /// rename alone would strand them under a name that no longer exists.
  const renameCategory = useCallback(
    (from: string, to: string) => {
      onSaveCategories(categories.map((c) => (c.name === from ? { ...c, name: to } : c)));
      for (const entry of entries) {
        if (entry.category === from) onSaveEntry(withEdits(entry, { category: to }));
      }
      if (selectedCategory === from) setSelectedCategory(to);
    },
    [categories, entries, onSaveCategories, onSaveEntry, selectedCategory]
  );

  /// Deleting moves the orphaned entries to the fallback rather than
  /// leaving them under a ghost name only search could reach.
  const deleteCategory = useCallback(
    (name: string) => {
      let next = categories.filter((c) => c.name !== name);
      const orphans = entries.filter((e) => e.category === name);
      if (orphans.length > 0 && !next.some((c) => c.name === FALLBACK_CATEGORY)) {
        next = [{ name: FALLBACK_CATEGORY, color: colorFor(FALLBACK_CATEGORY) }, ...next];
      }
      onSaveCategories(next);
      for (const entry of orphans) {
        onSaveEntry(withEdits(entry, { category: FALLBACK_CATEGORY }));
      }
      if (selectedCategory === name) setSelectedCategory(null);
    },
    [categories, colorFor, entries, onSaveCategories, onSaveEntry, selectedCategory]
  );

  /// One selection across the whole rail. The two groups used to combine,
  /// and the intersection was invisible: sitting on an empty Notes filter
  /// and clicking a category showed nothing of that category, which reads
  /// as entries having vanished, not as two filters at work.
  const selectType = useCallback((type: CredentialType | null) => {
    setSelectedType(type);
    if (type) setSelectedCategory(null);
  }, []);

  const selectCategory = useCallback((name: string | null) => {
    setSelectedCategory(name);
    if (name) setSelectedType(null);
  }, []);

  const typeCounts = useMemo(() => {
    const counts = new Map<CredentialType, number>();
    for (const e of entries) {
      const t = typeOf(e);
      counts.set(t, (counts.get(t) ?? 0) + 1);
    }
    return counts;
  }, [entries]);

  /// What the silo holds, by kind, in the header. Counts of one read as
  /// "1 card", not "1 cards", and a kind with none of them says nothing.
  const headerSummary = useMemo(() => {
    if (entries.length === 0) return "Logins, cards, identities, SSH keys and notes";
    return CREDENTIAL_TYPES.filter((type) => (typeCounts.get(type) ?? 0) > 0)
      .map((type) => {
        const count = typeCounts.get(type)!;
        const label = count === 1 ? TYPE_LABELS[type].singular : TYPE_LABELS[type].plural;
        // Only "SSH" is an initialism; the rest read better lower-case mid
        // sentence.
        return `${count} ${label === "SSH key" || label === "SSH keys" ? label : label.toLowerCase()}`;
      })
      .join(" · ");
  }, [entries.length, typeCounts]);

  const filtered = useMemo(() => {
    let list = entries;
    if (selectedType) {
      list = list.filter((e) => typeOf(e) === selectedType);
    }
    if (selectedCategory) {
      list = list.filter((e) => e.category === selectedCategory);
    }
    if (search.trim()) {
      const q = search.toLowerCase();
      list = list.filter((e) => searchTextFor(e).includes(q));
    }
    return [...list].sort((a, b) => a.service.localeCompare(b.service));
  }, [entries, search, selectedCategory, selectedType]);

  const selected = useMemo(
    () => filtered.find((e) => e.id === selectedId) ?? null,
    [filtered, selectedId]
  );

  // The detail pane is the only place a code is shown, so the clock runs
  // exactly while one is there.
  useEffect(() => {
    setCountingDown(Boolean(selected?.totp_secret) && !editing);
  }, [selected?.totp_secret, editing]);

  /// Secrets go through the Rust side rather than the webview's clipboard
  /// API: on Windows that keeps them out of Clipboard History, which writes
  /// to disk, and out of Cloud Clipboard, and clears them again after a
  /// minute or so.
  const copySecret = useCallback(async (text: string) => {
    await invoke("copy_secret_to_clipboard", { text });
  }, []);

  /// When each protected entry last passed a key touch. In-memory only and
  /// per entry: locking, switching silos or restarting always asks again.
  const verifiedAtRef = useRef<Map<string, number>>(new Map());
  const [verifying, setVerifying] = useState(false);

  /// A touch covers one entry for a few minutes, so copying the username,
  /// the password and the code is one touch, not three. Long enough to log
  /// into one site, short enough that walking away closes the window.
  const REAUTH_GRACE_MS = 3 * 60_000;

  /// Gate for everything a protected entry keeps behind a fresh touch.
  /// Entries without the flag pass straight through.
  const ensureVerified = useCallback(
    async (entry: PasswordEntry): Promise<boolean> => {
      if (!entry.require_reauth) return true;
      const last = verifiedAtRef.current.get(entry.id);
      if (last !== undefined && Date.now() - last < REAUTH_GRACE_MS) return true;

      setTransferError(null);
      setVerifying(true);
      try {
        await invoke("fido_reverify");
        verifiedAtRef.current.set(entry.id, Date.now());
        return true;
      } catch (e) {
        setTransferError(formatAppError(e));
        return false;
      } finally {
        setVerifying(false);
      }
    },
    [REAUTH_GRACE_MS]
  );

  const flashCopied = useCallback((key: string) => {
    setCopiedId(key);
    setTimeout(() => setCopiedId(null), 2000);
  }, []);

  /// Non-secret text: name, email, public key. The ordinary clipboard is
  /// fine for these, and clearing it would be theatre.
  const copyPlain = useCallback(
    async (key: string, text: string) => {
      await navigator.clipboard.writeText(text);
      flashCopied(key);
    },
    [flashCopied]
  );

  /// A secret belonging to `entry`: passes the entry's re-auth gate, then
  /// goes through the clearing clipboard.
  const copySecretField = useCallback(
    async (entry: PasswordEntry, key: string, text: string) => {
      if (!(await ensureVerified(entry))) return;
      await copySecret(text);
      flashCopied(key);
    },
    [copySecret, ensureVerified, flashCopied]
  );

  /// The list's one-click copy: whatever this kind of entry exists to hand
  /// over. Password, card number and the body of a note are secrets; an
  /// email address and a public key are exactly the parts meant to be given
  /// out.
  const copyPassword = useCallback(
    async (entry: PasswordEntry) => {
      const value = oneClickCopyValue(entry);
      // The route is a property of the kind of entry, decided in one place
      // and tested there. A note takes the secret route: it is free text
      // somebody chose to keep in a password manager, and the ordinary
      // clipboard on Windows writes what it holds to Clipboard History on
      // disk and syncs it to their other machines.
      if (copyKindFor(entry) === "secret") {
        await copySecretField(entry, entry.id, value);
      } else {
        await copyPlain(entry.id, value);
      }
    },
    [copyPlain, copySecretField]
  );

  const copyUsername = useCallback(
    async (entry: PasswordEntry) => {
      await copyPlain(`u-${entry.id}`, entry.username);
    },
    [copyPlain]
  );

  const copyTotp = useCallback(
    async (entry: PasswordEntry, code: string) => {
      if (!(await ensureVerified(entry))) return;
      await copySecret(code);
      flashCopied(`t-${entry.id}`);
    },
    [copySecret, ensureVerified, flashCopied]
  );

  const openAttachment = useCallback(
    async (entry: PasswordEntry, attachment: PasswordAttachment) => {
      if (!(await ensureVerified(entry))) return;
      // Attachments sync like everything else, so one can arrive from another
      // device. Same reasoning as opening a file in the explorer.
      if (runsOnOpen(attachment.name) && !(await onConfirmRun(attachment.name))) return;
      setTransferError(null);
      try {
        await invoke("password_open_attachment", {
          blobId: attachment.blob_id,
          name: attachment.name,
          blobKey: attachment.blob_key,
        });
      } catch (e) {
        setTransferError(formatAppError(e));
      }
    },
    [ensureVerified, onConfirmRun]
  );

  const startCreate = useCallback((type: CredentialType) => {
    setAddMenuOpen(false);
    setEditing({ entry: emptyEntry(type), creating: true });
  }, []);

  /// Editing is gated too: the editor's Show button and attachment list
  /// would otherwise be a one-click detour around the reveal gate.
  const startEdit = useCallback(
    async (entry: PasswordEntry) => {
      if (!(await ensureVerified(entry))) return;
      setEditing({ entry: { ...entry }, creating: false });
    },
    [ensureVerified]
  );

  const handleSave = useCallback(
    (entry: PasswordEntry) => {
      onSaveEntry(withEdits(entry, { updated_at: Date.now() }));
      setEditing(null);
      setSelectedId(entry.id);
      // A filter that would hide what was just saved gets out of the way:
      // adding an SSH key while looking at Cards must not end in a save
      // that appears to have vanished.
      setSelectedType((prev) => (prev && typeOf(entry) !== prev ? null : prev));
      setSelectedCategory((prev) => (prev && entry.category !== prev ? null : prev));
    },
    [onSaveEntry]
  );

  /// Deleting a credential has no trash behind it, so the confirmation names
  /// the entry: the user should be answering about this login, not about
  /// whichever one the pane happened to be showing.
  const [pendingDelete, setPendingDelete] = useState<PasswordEntry | null>(null);

  const confirmDelete = useCallback(() => {
    if (!pendingDelete) return;
    onDeleteEntry(pendingDelete.id);
    setPendingDelete(null);
    setSelectedId(null);
  }, [onDeleteEntry, pendingDelete]);

  const handleImport = useCallback(async () => {
    setTransferError(null);
    const picked = await openFileDialog({
      multiple: false,
      filters: [{ name: "Password export (CSV or Bitwarden JSON)", extensions: ["csv", "json"] }],
    });
    const path = typeof picked === "string" ? picked : picked?.[0];
    if (!path) return;

    setTransferBusy(true);
    try {
      const text = await invoke<string>("passwords_read_import_csv", { path });
      let imported: PasswordEntry[];
      let skipped: number;
      let source: string;
      let unit: [string, string];
      let skippedUnit: [string, string];

      if (looksLikeBitwardenJson(text)) {
        ({ entries: imported, skipped } = bitwardenJsonToEntries(text));
        source = "Bitwarden JSON";
        unit = ["item", "items"];
        skippedUnit = ["unsupported item", "unsupported items"];
      } else {
        const parsed = csvToEntries(text);
        imported = parsed.entries;
        skipped = parsed.skipped;
        source = formatLabel(parsed.format);
        unit = ["login", "logins"];
        skippedUnit = ["non-login row", "non-login rows"];
      }

      // Parsed but not yet stored: the user first says where these get
      // filed. Nothing is written until they confirm.
      setPendingImport({ imported, skipped, source, unit, skippedUnit });
    } catch (e) {
      setTransferError(formatAppError(e));
    } finally {
      setTransferBusy(false);
    }
  }, []);

  const finishImport = useCallback(
    (choice: ImportCategoryChoice) => {
      if (!pendingImport) return;
      const { skipped, source, unit, skippedUnit } = pendingImport;
      const imported = applyImportCategory(pendingImport.imported, choice);
      setPendingImport(null);

      // Appended, never merged over what's already there. An import that
      // silently overwrote existing entries would be unrecoverable, so an
      // entry whose content changed since the last export still arrives as a
      // second copy; only an exact match is dropped as a duplicate.
      const { fresh, duplicates } = dropDuplicates(entries, imported);
      if (fresh.length > 0) onImportEntries(fresh);

      const plural = (n: number, [one, many]: [string, string]) => (n === 1 ? one : many);
      const notes: string[] = [];
      if (duplicates > 0) notes.push(`skipped ${duplicates} already in this silo`);
      if (skipped > 0) notes.push(`skipped ${skipped} ${plural(skipped, skippedUnit)}`);
      const suffix = notes.length > 0 ? `, ${notes.join(", ")}` : "";

      setTransferNotice(
        fresh.length === 0 && duplicates > 0
          ? `Nothing new in that file: all ${duplicates} ${plural(duplicates, unit)} are already in this silo.`
          : `Imported ${fresh.length} ${plural(fresh.length, unit)} from ${source}${suffix}.`
      );
    },
    [entries, onImportEntries, pendingImport]
  );

  const handleExport = useCallback(async () => {
    setTransferError(null);

    // The CSV dialect other managers read has columns for logins only.
    const logins = entries.filter((e) => typeOf(e) === "login");

    // An export writes every one of these into a plaintext file, which is
    // the broadest reveal in the app. Entries marked "ask again before
    // revealing" were the one thing it did not ask about: the flag covered
    // copying, editing and opening an attachment, and then the export
    // handed the same secrets over with no touch at all. Asked once for the
    // batch rather than per entry, and asked before the file dialog, so
    // somebody who cannot produce the key is not first made to choose where
    // to put a file that is not going to be written.
    if (exportNeedsTouch(logins)) {
      const asking = logins.find((e) => e.require_reauth)!;
      if (!(await ensureVerified(asking))) return;
    }

    const path = await saveFileDialog({
      defaultPath: "silentsilo-passwords.csv",
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!path) return;

    setTransferBusy(true);
    try {
      await invoke("passwords_write_export_csv", { path, contents: entriesToCsv(logins) });
      const leftOut =
        entries.length - logins.length > 0
          ? ` Cards, identities and SSH keys (${entries.length - logins.length}) are not part of the CSV format and stayed behind.`
          : "";
      setTransferNotice(
        `Exported ${logins.length} ${logins.length === 1 ? "login" : "logins"} as an unencrypted CSV file. Store or delete it carefully.${leftOut}`
      );
    } catch (e) {
      setTransferError(formatAppError(e));
    } finally {
      setTransferBusy(false);
      setConfirmingExport(false);
    }
  }, [ensureVerified, entries]);

  return (
    <div className="pw-view">
      <ViewHeader icon={KeyRound} title="Credentials" subtitle={headerSummary} />
      {/* Toolbar spans all three panes: search and transfer act on the whole
          store, not on any one pane. */}
      <div className="view-toolbar">
        <div className="view-search">
          <span className="search-icon">
            <IconSearch size={16} />
          </span>
          <input
            type="text"
            placeholder="Search passwords…"
            aria-label="Search passwords"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <button
          type="button"
          className={`pw-favicon-toggle${showFavicons ? " active" : ""}`}
          onClick={toggleShowFavicons}
          title={
            showFavicons
              ? "Site icons are on. Each saved site sees your IP address whenever this list renders. Click to turn them off."
              : "Site icons are off. Turning them on lets each saved site see your IP address whenever this list renders."
          }
        >
          {showFavicons ? <IconEye size={15} /> : <IconEyeOff size={15} />}
          <span>Site icons</span>
        </button>
        <button
          type="button"
          className="pw-transfer-btn"
          disabled={busy || transferBusy}
          onClick={() => void handleImport()}
          title="Import logins from another password manager or browser: CSV from Bitwarden, LastPass, 1Password, Proton Pass, Dashlane, NordPass, KeePass, RoboForm, Chrome, Edge, Firefox or Apple Passwords, plus Bitwarden JSON"
        >
          <Upload size={15} />
          <span>Import</span>
        </button>
        <button
          type="button"
          className="pw-transfer-btn"
          disabled={busy || transferBusy || entries.length === 0}
          onClick={() => {
            setTransferError(null);
            setTransferNotice(null);
            setConfirmingExport(true);
          }}
          title="Export all logins as a CSV file"
        >
          <Download size={15} />
          <span>Export</span>
        </button>
        <div className="pw-add-wrap">
          <button
            type="button"
            className="btn-add-password"
            disabled={busy}
            aria-expanded={addMenuOpen}
            onClick={() => setAddMenuOpen((v) => !v)}
          >
            <IconPlus size={16} />
            <span>Add item</span>
          </button>
          {addMenuOpen && (
            <>
              <div className="dropdown-overlay" onClick={() => setAddMenuOpen(false)} />
              <div className="add-dropdown-menu" role="menu">
                {CREDENTIAL_TYPES.map((type) => {
                  const Icon = TYPE_ICONS[type];
                  return (
                    <button
                      key={type}
                      type="button"
                      className="dropdown-item"
                      role="menuitem"
                      onClick={() => startCreate(type)}
                    >
                      <Icon size={16} />
                      <span>{TYPE_LABELS[type].singular}</span>
                    </button>
                  );
                })}
              </div>
            </>
          )}
        </div>
      </div>

      {pendingImport && (
        <ImportFilingDialog
          what={`${pendingImport.imported.length} ${
            pendingImport.imported.length === 1 ? pendingImport.unit[0] : pendingImport.unit[1]
          } from ${pendingImport.source}.`}
          categories={categories.map((c) => c.name)}
          onConfirm={finishImport}
          onCancel={() => setPendingImport(null)}
        />
      )}

      {confirmingExport && (
        <div className="pw-export-warning" role="alertdialog" aria-label="Confirm plaintext export">
          <div>
            <strong>This export is not encrypted.</strong>
            <p>
              Every password and TOTP secret is written to a readable file so other password
              managers can import it. Anyone who opens that file can read your logins, as can any
              backup or sync tool that picks it up. Save it somewhere you control, and delete it
              once you are done.
            </p>
          </div>
          <div className="pw-export-warning-actions">
            <button type="button" className="secondary" onClick={() => setConfirmingExport(false)}>
              Cancel
            </button>
            <button type="button" disabled={transferBusy} onClick={() => void handleExport()}>
              Export anyway
            </button>
          </div>
        </div>
      )}

      {transferNotice && (
        <div className="pw-transfer-notice" role="status">
          <span>{transferNotice}</span>
          <button type="button" className="link" onClick={() => setTransferNotice(null)}>
            Dismiss
          </button>
        </div>
      )}

      {transferError && (
        <div className="pw-transfer-notice is-error" role="alert">
          <span>{transferError}</span>
          <button type="button" className="link" onClick={() => setTransferError(null)}>
            Dismiss
          </button>
        </div>
      )}

      {verifying && (
        <div className="pw-transfer-notice" role="status">
          <span>Touch your security key, or confirm with Windows Hello…</span>
        </div>
      )}

      {editLockNoticeUp && (
        <div className="pw-transfer-notice" role="status">
          <span>An entry is being edited. Save it or press Cancel before opening another.</span>
        </div>
      )}

      {entries.length === 0 && !editing ? (
        <div className="empty-state">
          <ShieldCheck size={48} className="empty-icon" />
          <p className="empty-title">Nothing here yet</p>
          {/* Every kind on offer, up front: a single "Add login" made the
              other three discoverable only through a menu nobody has opened
              yet. */}
          <div className="pw-empty-choices">
            {CREDENTIAL_TYPES.map((type) => {
              const Icon = TYPE_ICONS[type];
              return (
                <button
                  key={type}
                  type="button"
                  className="pw-empty-choice"
                  disabled={busy}
                  onClick={() => startCreate(type)}
                >
                  <Icon size={18} />
                  <span>{TYPE_LABELS[type].singular}</span>
                </button>
              );
            })}
          </div>
          <p className="hint">
            Or use Import to bring everything over from Bitwarden, LastPass, 1Password or Chrome.
          </p>
        </div>
      ) : (
        <div className="pw-layout">
          <CategoryRail
            categories={categories}
            counts={categoryCounts}
            total={entries.length}
            selected={selectedCategory}
            typeCounts={typeCounts}
            selectedType={selectedType}
            onSelectType={selectType}
            busy={busy}
            onSelect={selectCategory}
            onAdd={(category) => onSaveCategories([...categories, category])}
            onRename={renameCategory}
            onDelete={deleteCategory}
          />

          <div className="pw-list-pane">
            {filtered.length === 0 ? (
              search.trim() ? (
                // A search that found nothing is not an invitation to add:
                // the user is looking for something they believe exists.
                <div className="empty-state">
                  <IconSearch size={36} className="empty-icon" />
                  <p className="empty-title">No matching items</p>
                  <p className="hint">Try a different search, kind or category.</p>
                </div>
              ) : selectedType ? (
                (() => {
                  const Icon = TYPE_ICONS[selectedType];
                  return (
                    <div className="empty-state">
                      <Icon size={36} className="empty-icon" />
                      <p className="empty-title">
                        No {TYPE_LABELS[selectedType].plural.toLowerCase()} yet
                      </p>
                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => startCreate(selectedType)}
                      >
                        <IconPlus size={15} />
                        Add {TYPE_LABELS[selectedType].singular.toLowerCase()}
                      </button>
                    </div>
                  );
                })()
              ) : (
                <div className="empty-state">
                  <ShieldCheck size={36} className="empty-icon" />
                  <p className="empty-title">Nothing in this category yet</p>
                  <p className="hint">Use Add item, or pick another category.</p>
                </div>
              )
            ) : (
              <EntryList
                entries={filtered}
                selectedId={selected?.id ?? null}
                showFavicons={showFavicons}
                copiedId={copiedId}
                colorFor={colorFor}
                onSelect={(id) => {
                  // Switching entries while editing would silently discard
                  // the draft, so an explicit Cancel is required first. Said
                  // out loud: a click that does nothing reads as a bug.
                  if (editing) flashEditLock();
                  else setSelectedId(id);
                }}
                onCopyUsername={(entry) => void copyUsername(entry)}
                onCopyPassword={(entry) => void copyPassword(entry)}
              />
            )}
          </div>

          <div className="pw-detail-pane">
            {editing ? (
              <EntryEditor
                initial={editing.entry}
                creating={editing.creating}
                categories={categories}
                now={now}
                onSave={handleSave}
                onCancel={() => setEditing(null)}
              />
            ) : selected ? (
              /* Keyed on the entry, so React builds a new detail pane for
                 each one rather than handing the next entry the state of
                 the last. Without it a reveal survived the selection
                 changing: reveal one entry, click the next, and its
                 password, card number and security code were on screen
                 already, including for an entry that asks for a key touch
                 before it is shown. */
              <EntryDetail
                key={selected.id}
                entry={selected}
                now={now}
                showFavicons={showFavicons}
                copiedId={copiedId}
                colorFor={colorFor}
                busy={busy}
                onCopyUsername={(entry) => void copyUsername(entry)}
                onCopyTotp={(entry, code) => void copyTotp(entry, code)}
                onCopyPlain={(key, text) => void copyPlain(key, text)}
                onCopySecretField={(entry, key, text) => void copySecretField(entry, key, text)}
                onOpenAttachment={(attachment) => void openAttachment(selected, attachment)}
                onRequestReveal={ensureVerified}
                onToggleFavorite={(entry) => onSaveEntry(withEdits(entry, { favorite: !entry.favorite }))}
                onEdit={(entry) => void startEdit(entry)}
                onDelete={() => setPendingDelete(selected)}
              />
            ) : filtered.length > 0 ? (
              <div className="pw-detail-placeholder">
                <MousePointerClick size={32} className="empty-icon" />
                <p className="hint">Select an item to see its details.</p>
              </div>
            ) : // An empty list already says everything; a second pane
            // repeating "select something" would be advice about nothing.
            null}
          </div>
        </div>
      )}

      {pendingDelete && (
        <ConfirmDialog
          title="Delete this item?"
          message={`"${pendingDelete.service}" is removed from every device on the next sync. Items deleted here do not go to the trash.`}
          confirmLabel="Delete"
          danger
          busy={busy}
          onConfirm={confirmDelete}
          onCancel={() => setPendingDelete(null)}
        />
      )}
    </div>
  );
}
