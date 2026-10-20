import { useState, useMemo, useCallback, useEffect, useRef } from "react";
import type { Os } from "../../lib/platformStrings";
import { invoke } from "@tauri-apps/api/core";
import { open as openFileDialog, save as saveFileDialog } from "../../lib/dialog";
import {
  Download,
  FolderInput,
  KeyRound,
  ListChecks,
  MousePointerClick,
  ShieldCheck,
  Star,
  Trash2,
  Upload,
} from "lucide-react";
import { ViewHeader } from "../../components/ViewHeader";
import type {
  CredentialType,
  EntryChange,
  HistoryVersion,
  PasswordAttachment,
  PasswordCategory,
  PasswordEntry,
} from "../../lib/types";
import {
  csvToEntries,
  entriesToCsv,
  formatDisplayName,
  formatLabel,
} from "../../lib/passwordCsv";
import {
  attachZipFiles,
  bitwardenJsonToEntries,
  looksLikeBitwardenJson,
  type ZipFile,
} from "../../lib/bitwardenJson";
import {
  applyImportCategory,
  describeExtras,
  dropDuplicates,
  noExtras,
  type ImportCategoryChoice,
  type ImportExtras,
} from "../../lib/passwordImport";
import { withEdits } from "../../lib/passwordEntry";
import { restoredFrom, withBaseFields, withoutHistory } from "../../lib/entryHistory";
import { formatAppError } from "../../lib/errors";
import { runsOnOpen } from "../../lib/executable";
import { ConfirmDialog } from "../ConfirmDialog";
import { EntryList } from "./EntryList";
import { ImportFilingDialog } from "./ImportFilingDialog";
import { KdbxPasswordDialog } from "./KdbxPasswordDialog";
import { ExportChoiceDialog } from "./ExportChoiceDialog";
import { PasswordTrashDialog, type TrashedEntry } from "./PasswordTrashDialog";
import { attachmentBlobs, kdbxToEntries, type KdbxEntry } from "../../lib/kdbx";
import { loadHistoryPolicy } from "../../lib/historySetting";
import { EntryDetail } from "./EntryDetail";
import { EntryEditor } from "./EntryEditor";
import { CategoryRail, TYPE_ICONS } from "./CategoryRail";
import {
  copyKindFor,
  CREDENTIAL_TYPES,
  DEFAULT_GEN_OPTIONS,
  FALLBACK_CATEGORY,
  generatePassword,
  makeColorFor,
  oneClickCopyValue,
  oneClickField,
  resolveCategories,
  searchTextFor,
  TYPE_LABELS,
  TYPE_TEXTS,
  typeOf,
} from "./util";
import { IconClose, IconEye, IconEyeOff, IconPlus, IconSearch, IconTrash } from "../../ui/Icons";
import { t, useLocale, type Key } from "../../i18n";
import { useLasting, useLastingCheck } from "../../lib/lasting";

type Props = {
  entries: PasswordEntry[];
  /** The stored category list, or null when this silo never saved one. */
  storedCategories: PasswordCategory[] | null;
  os: Os;
  /** The key prompt the backend sent, worded for this silo's keys. */
  fidoProgress?: string | null;
  busy: boolean;
  /** Whether this silo has backup storage, so texts about syncing and
   * storage apply at all. */
  backedUp: boolean;
  /** Copies the app never deletes from, which keep a deleted entry's
   * attachments. */
  archiveTargets: number;
  /** An entry another view is sending the user to. Selecting it clears the
   * filters, or the panel would land on an entry the current category or
   * search hides, and show nothing. */
  focusEntryId?: string | null;
  /** Creates or replaces one entry. */
  /** Resolves to whether the entry was stored. */
  onSaveEntry: (entry: PasswordEntry, change?: EntryChange) => Promise<boolean>;
  onDeleteEntry: (entry: PasswordEntry) => void;
  /** Protected entries a key touch just let through, whole, to hold in
   * place of the copies read without their secrets. */
  onRevealed: (entries: PasswordEntry[]) => void;
  onImportEntries: (entries: PasswordEntry[], source: string) => void;
  /** Replaces the category list as a whole. */
  onSaveCategories: (categories: PasswordCategory[]) => void;
  /** Asks the user to confirm something that runs when opened. */
  onConfirmRun: (name: string) => Promise<boolean>;
  /** Puts an entry back from the password trash. */
  onRestoreEntry: (entry: TrashedEntry) => Promise<void>;
};

const SHOW_FAVICONS_KEY = "silentsilo.passwords.showFavicons";

/** The extension of a SilentSilo password export, as core names it. */
const SILENTSILO_EXPORT = "silopwd";

/** An export's entries, each with a new id where this silo already has the
 *  id: an import adds, it never overwrites an entry that is here. */
function withFreshIds(imported: PasswordEntry[], here: PasswordEntry[]): PasswordEntry[] {
  const taken = new Set(here.map((e) => e.id));
  return imported.map((e) => (taken.has(e.id) ? { ...e, id: crypto.randomUUID() } : e));
}

/** What an import counts, by where it came from: Bitwarden items, CSV
 * logins or KeePass entries, each with what its skipped rows were. */
type ImportKind = "item" | "login" | "entry";

const IMPORT_TEXTS: Record<
  ImportKind,
  { what: Key; imported: Key; importedWith: Key; nothingNew: Key; skipped: Key }
> = {
  item: {
    what: "pw.import_what_item",
    imported: "pw.imported_item",
    importedWith: "pw.imported_item_with",
    nothingNew: "pw.nothing_new_item",
    skipped: "pw.skip_unsupported",
  },
  login: {
    what: "pw.import_what_login",
    imported: "pw.imported_login",
    importedWith: "pw.imported_login_with",
    nothingNew: "pw.nothing_new_login",
    skipped: "pw.skip_non_login",
  },
  entry: {
    what: "pw.import_what_entry",
    imported: "pw.imported_entry",
    importedWith: "pw.imported_entry_with",
    nothingNew: "pw.nothing_new_entry",
    skipped: "pw.skip_empty",
  },
};

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
const never = () => false;

export function PasswordsPanel({
  entries,
  storedCategories,
  os,
  fidoProgress = null,
  busy,
  backedUp,
  archiveTargets,
  focusEntryId,
  onSaveEntry,
  onDeleteEntry,
  onRevealed,
  onImportEntries,
  onSaveCategories,
  onConfirmRun,
  onRestoreEntry,
}: Props) {
  useLocale();
  const [search, setSearch] = useState("");
  const [selectedCategory, setSelectedCategory] = useState<string | null>(null);
  const [selectedType, setSelectedType] = useState<CredentialType | null>(null);
  const [addMenuOpen, setAddMenuOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ entry: PasswordEntry; creating: boolean } | null>(null);
  // Lasting: an import goes on when the view is left, and one parsed and
  // waiting to be filed must still be there, or its attachments, already
  // encrypted into the silo, are never cleaned up. See lib/lasting.
  const [transferBusy, setTransferBusy] = useLasting<boolean>("pw.transfer_busy", false, Boolean);
  const [transferNotice, setTransferNotice] = useLasting<string | null>(
    "pw.transfer_notice",
    null,
    never,
  );
  const [transferError, setTransferError] = useLasting<string | null>(
    "pw.transfer_error",
    null,
    never,
  );
  const [confirmingExport, setConfirmingExport] = useState(false);
  /// The ticked entries when the export was asked for from the selection
  /// bar; null exports the whole store.
  const [exportOnly, setExportOnly] = useState<PasswordEntry[] | null>(null);
  const toExport = exportOnly ?? entries;
  /** Parsed and counted, waiting for the user to say where it gets filed. */
  const [pendingImport, setPendingImport] = useLasting<{
    imported: PasswordEntry[];
    skipped: number;
    /** For the activity log, in English. */
    source: string;
    /** For the notice on screen. */
    sourceLabel: string;
    kind: ImportKind;
    extras: ImportExtras;
    /** Attachments a KeePass import already encrypted into the silo, which
     * a cancel deletes again. */
    blobs?: string[];
    /** A SilentSilo export's categories, with their colours. */
    categories?: PasswordCategory[];
  } | null>("pw.pending_import", null, Boolean);
  /** A KeePass database being opened for import, or the export's password
   * being asked for. */
  const [kdbx, setKdbx] = useState<{
    mode: "open" | "export";
    path: string;
    format: "kdbx" | "silentsilo";
  } | null>(null);
  const [kdbxError, setKdbxError] = useState<string | null>(null);
  /// An import read before a lock must not land after it: what it parsed
  /// is passwords in clear.
  const startedFor = useLastingCheck();
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
        if (entry.category === from) onSaveEntry(withEdits(entry, { category: to }), "arranged");
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
        onSaveEntry(withEdits(entry, { category: FALLBACK_CATEGORY }), "arranged");
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
      const kind = typeOf(e);
      counts.set(kind, (counts.get(kind) ?? 0) + 1);
    }
    return counts;
  }, [entries]);

  /// What the silo holds, by kind, in the header. Counts of one read as
  /// "1 card", not "1 cards", and a kind with none of them says nothing.
  /// Not memoised: it follows the language too, and costs nothing.
  const headerSummary =
    entries.length === 0
      ? t("pw.header_kinds")
      : CREDENTIAL_TYPES.filter((type) => (typeCounts.get(type) ?? 0) > 0)
          .map((type) => t(TYPE_TEXTS[type].count, { count: typeCounts.get(type)! }))
          .join(" · ");

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
  /// `field` names what was copied for the silo's activity log, which
  /// records it before the clipboard holds anything.
  const copySecret = useCallback(async (entry: PasswordEntry, text: string, field: string) => {
    await invoke("copy_secret_to_clipboard", {
      text,
      audit: { entry_id: entry.id, label: entry.service, field },
    });
  }, []);

  /// When each protected entry last passed a key touch. In-memory only and
  /// per entry: locking, switching silos or restarting always asks again.
  const verifiedAtRef = useRef<Map<string, number>>(new Map());
  const [verifying, setVerifying] = useState(false);

  /// A touch covers one entry for a few minutes, so copying the username,
  /// the password and the code is one touch, not three. Long enough to log
  /// into one site, short enough that walking away closes the window.
  const REAUTH_GRACE_MS = 3 * 60_000;

  /// The key touch for protected entries. The backend hands them back
  /// whole, since the list it gives the window holds them without their
  /// secrets. Null when the touch did not happen.
  const reveal = useCallback(
    async (asking: PasswordEntry[], purpose?: "change"): Promise<PasswordEntry[] | null> => {
      setTransferError(null);
      setVerifying(true);
      try {
        const rows = await invoke<string[]>("fido_reverify", {
          purpose: purpose ?? null,
          entries: asking.map((e) => e.id),
        });
        const whole = rows.map((row) => withBaseFields(JSON.parse(row) as PasswordEntry));
        const now = Date.now();
        for (const entry of asking) verifiedAtRef.current.set(entry.id, now);
        if (whole.length > 0) onRevealed(whole);
        const byId = new Map(whole.map((e) => [e.id, e]));
        return asking.map((e) => byId.get(e.id) ?? e);
      } catch (e) {
        setTransferError(formatAppError(e));
        return null;
      } finally {
        setVerifying(false);
      }
    },
    [onRevealed, setTransferError]
  );

  /// Whether a protected entry is still covered by its last touch, and
  /// held whole.
  const coveredNow = useCallback(
    (entry: PasswordEntry): boolean => {
      if (!entry.require_reauth) return true;
      const last = verifiedAtRef.current.get(entry.id);
      return !entry.withheld && last !== undefined && Date.now() - last < REAUTH_GRACE_MS;
    },
    [REAUTH_GRACE_MS]
  );

  /// Gate for everything a protected entry keeps behind a fresh touch.
  /// Entries without the flag pass straight through. Resolves to the entry
  /// whole, or null when the touch did not happen.
  const ensureVerified = useCallback(
    async (entry: PasswordEntry): Promise<PasswordEntry | null> => {
      if (coveredNow(entry)) return entry;
      const whole = await reveal([entry]);
      return whole?.[0] ?? null;
    },
    [coveredNow, reveal]
  );

  /// The same for several entries at once, for one act on all of them
  /// (deleting, moving): one touch covers the protected ones among them.
  const ensureVerifiedAll = useCallback(
    async (list: PasswordEntry[]): Promise<PasswordEntry[] | null> => {
      const asking = list.filter((e) => !coveredNow(e));
      if (asking.length === 0) return list;
      const whole = await reveal(asking, "change");
      if (!whole) return null;
      const byId = new Map(whole.map((e) => [e.id, e]));
      return list.map((e) => byId.get(e.id) ?? e);
    },
    [coveredNow, reveal]
  );

  /// Showing an entry's secrets: the gate, then the activity log, which an
  /// organisation's silo may not do without.
  const requestReveal = useCallback(
    async (entry: PasswordEntry): Promise<boolean> => {
      if (!(await ensureVerified(entry))) return false;
      try {
        await invoke("audit_note", {
          note: "entry_revealed",
          entryId: entry.id,
          label: entry.service,
        });
        return true;
      } catch (e) {
        setTransferError(formatAppError(e));
        return false;
      }
    },
    [ensureVerified, setTransferError]
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
  /// goes through the clearing clipboard. `pick` reads it from the entry
  /// whole, which a protected one only is after the touch.
  const copySecretField = useCallback(
    async (
      entry: PasswordEntry,
      key: string,
      pick: (whole: PasswordEntry) => string,
      field: string,
    ) => {
      const whole = await ensureVerified(entry);
      if (!whole) return;
      await copySecret(whole, pick(whole), field);
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
        await copySecretField(entry, entry.id, oneClickCopyValue, oneClickField(entry));
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
      await copySecret(entry, code, "one-time code");
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
          entryLabel: entry.service,
        });
      } catch (e) {
        setTransferError(formatAppError(e));
      }
    },
    [ensureVerified, onConfirmRun, setTransferError]
  );

  const startCreate = useCallback((type: CredentialType) => {
    setAddMenuOpen(false);
    setEditing({ entry: emptyEntry(type), creating: true });
  }, []);

  /// Editing is gated too: the editor's Show button and attachment list
  /// would otherwise be a one-click detour around the reveal gate.
  const startEdit = useCallback(
    async (entry: PasswordEntry) => {
      const whole = await ensureVerified(entry);
      if (!whole) return;
      setEditing({ entry: { ...whole }, creating: false });
    },
    [ensureVerified]
  );

  /// The editor stays open when the save fails, so the draft is not lost.
  const handleSave = useCallback(
    async (entry: PasswordEntry): Promise<boolean> => {
      if (!(await onSaveEntry(withEdits(entry, { updated_at: Date.now() })))) return false;
      setEditing(null);
      setSelectedId(entry.id);
      // A filter that would hide what was just saved gets out of the way:
      // adding an SSH key while looking at Cards must not end in a save
      // that appears to have vanished.
      setSelectedType((prev) => (prev && typeOf(entry) !== prev ? null : prev));
      setSelectedCategory((prev) => (prev && entry.category !== prev ? null : prev));
      return true;
    },
    [onSaveEntry]
  );

  /// Deleting a credential has no trash behind it, so the confirmation names
  /// the entry: the user should be answering about this login, not about
  /// whichever one the pane happened to be showing.
  const [pendingDelete, setPendingDelete] = useState<PasswordEntry | null>(null);

  /// Entries ticked for one action on all of them: moved to a category,
  /// starred, deleted. Kept to what the list shows, so a search or another
  /// category never leaves an action reaching entries out of sight.
  const [checked, setChecked] = useState<ReadonlySet<string>>(() => new Set());
  const checkAnchor = useRef<string | null>(null);
  const [movingChecked, setMovingChecked] = useState(false);
  const [pendingBulkDelete, setPendingBulkDelete] = useState(false);
  const [trashOpen, setTrashOpen] = useState(false);
  /// Clearing takes old passwords out of the entry on every device; older
  /// records of the silo keep them until those are cleared.
  const [pendingClearHistory, setPendingClearHistory] = useState<PasswordEntry | null>(null);

  /// A restore changes the secret, so it is gated like an edit. Not asked
  /// about: the current version goes into the history, so it can be undone.
  const restoreVersion = useCallback(
    async (entry: PasswordEntry, version: HistoryVersion) => {
      const whole = await ensureVerified(entry);
      if (!whole) return;
      await onSaveEntry(restoredFrom(whole, version, Date.now()), "restored");
    },
    [ensureVerified, onSaveEntry]
  );

  const visibleIds = useMemo(() => new Set(filtered.map((e) => e.id)), [filtered]);
  useEffect(() => {
    setChecked((prev) => {
      const kept = [...prev].filter((id) => visibleIds.has(id));
      return kept.length === prev.size ? prev : new Set(kept);
    });
  }, [visibleIds]);

  const checkEntry = useCallback(
    (id: string, range: boolean) => {
      setChecked((prev) => {
        const next = new Set(prev);
        const from = checkAnchor.current ? filtered.findIndex((e) => e.id === checkAnchor.current) : -1;
        const to = filtered.findIndex((e) => e.id === id);
        if (range && from >= 0 && to >= 0) {
          const [a, b] = from < to ? [from, to] : [to, from];
          for (const entry of filtered.slice(a, b + 1)) next.add(entry.id);
        } else if (next.has(id)) {
          next.delete(id);
        } else {
          next.add(id);
        }
        return next;
      });
      checkAnchor.current = id;
    },
    [filtered]
  );

  /// Every entry the list shows now, after the search and the filters.
  const checkAll = useCallback(() => setChecked(new Set(filtered.map((e) => e.id))), [filtered]);

  const clearChecked = useCallback(() => {
    setChecked(new Set());
    setMovingChecked(false);
    checkAnchor.current = null;
  }, []);

  const checkedEntries = useMemo(
    () => entries.filter((e) => checked.has(e.id)),
    [checked, entries]
  );
  const allStarred = checkedEntries.length > 0 && checkedEntries.every((e) => e.favorite);

  // One at a time: each is its own change in the silo's history. A
  // protected entry among them asks for the key first, once for all.
  const moveChecked = useCallback(
    async (category: string) => {
      setMovingChecked(false);
      const moving = checkedEntries.filter((entry) => entry.category !== category);
      if (!(await ensureVerifiedAll(moving))) return;
      for (const entry of moving) {
        await onSaveEntry(withEdits(entry, { category }), "arranged");
      }
      clearChecked();
    },
    [checkedEntries, clearChecked, ensureVerifiedAll, onSaveEntry]
  );

  const starChecked = useCallback(async () => {
    const favorite = !allStarred;
    for (const entry of checkedEntries) {
      if (Boolean(entry.favorite) !== favorite) {
        await onSaveEntry(withEdits(entry, { favorite }), "arranged");
      }
    }
  }, [allStarred, checkedEntries, onSaveEntry]);

  /// A protected entry is deleted only after the key: the trash can put it
  /// back, but the delete still reaches every device.
  const confirmBulkDelete = useCallback(async () => {
    setPendingBulkDelete(false);
    const going = await ensureVerifiedAll(checkedEntries);
    if (!going) return;
    for (const entry of going) onDeleteEntry(entry);
    if (selectedId && checked.has(selectedId)) setSelectedId(null);
    clearChecked();
  }, [checked, checkedEntries, clearChecked, ensureVerifiedAll, onDeleteEntry, selectedId]);

  const confirmDelete = useCallback(async () => {
    if (!pendingDelete) return;
    setPendingDelete(null);
    const going = await ensureVerifiedAll([pendingDelete]);
    if (!going) return;
    onDeleteEntry(going[0]!);
    setSelectedId(null);
  }, [ensureVerifiedAll, onDeleteEntry, pendingDelete]);

  /// Bitwarden's ".zip (With Attachments)": its JSON, as any Bitwarden
  /// import, with the files already encrypted into the silo.
  const importBitwardenZip = useCallback(async (path: string) => {
    setTransferBusy(true);
    const current = startedFor();
    let blobs: string[] = [];
    try {
      const read = await invoke<{ json: string; files: ZipFile[] }>(
        "passwords_read_bitwarden_zip",
        { path },
      );
      blobs = read.files.map((f) => f.attachment.blob_id);
      if (!current()) return;
      const parsed = bitwardenJsonToEntries(read.json);
      const { entries: imported, unmatched } = attachZipFiles(parsed.entries, read.files);
      setPendingImport({
        imported,
        skipped: parsed.skipped,
        source: "Bitwarden",
        sourceLabel: "Bitwarden",
        kind: "item",
        extras: { ...parsed.extras, unmatchedFiles: unmatched },
        blobs,
      });
    } catch (e) {
      for (const blobId of blobs) {
        void invoke("password_delete_attachment", { blobId }).catch(() => {});
      }
      setTransferError(formatAppError(e));
    } finally {
      setTransferBusy(false);
    }
  }, [setPendingImport, setTransferBusy, setTransferError, startedFor]);

  const handleImport = useCallback(async () => {
    setTransferError(null);
    const picked = await openFileDialog({
      multiple: false,
      filters: [
        {
          name: t("pw.import_filter"),
          extensions: [SILENTSILO_EXPORT, "kdbx", "csv", "json", "zip"],
        },
      ],
    });
    const path = typeof picked === "string" ? picked : picked?.[0];
    if (!path) return;
    if (path.toLowerCase().endsWith(".kdbx")) {
      setKdbxError(null);
      setKdbx({ mode: "open", path, format: "kdbx" });
      return;
    }
    if (path.toLowerCase().endsWith(`.${SILENTSILO_EXPORT}`)) {
      setKdbxError(null);
      setKdbx({ mode: "open", path, format: "silentsilo" });
      return;
    }

    if (path.toLowerCase().endsWith(".zip")) {
      await importBitwardenZip(path);
      return;
    }

    setTransferBusy(true);
    const current = startedFor();
    try {
      const text = await invoke<string>("passwords_read_import_csv", { path });
      if (!current()) return;
      let imported: PasswordEntry[];
      let skipped: number;
      let source: string;
      let sourceLabel: string;
      let kind: ImportKind;
      let extras: ImportExtras;

      if (looksLikeBitwardenJson(text)) {
        ({ entries: imported, skipped, extras } = bitwardenJsonToEntries(text));
        source = "Bitwarden JSON";
        sourceLabel = source;
        kind = "item";
      } else {
        const parsed = csvToEntries(text);
        imported = parsed.entries;
        skipped = parsed.skipped;
        extras = parsed.extras;
        source = formatLabel(parsed.format);
        sourceLabel = formatDisplayName(parsed.format);
        kind = "login";
      }

      // Parsed but not yet stored: the user first says where these get
      // filed. Nothing is written until they confirm.
      setPendingImport({ imported, skipped, source, sourceLabel, kind, extras });
    } catch (e) {
      setTransferError(formatAppError(e));
    } finally {
      setTransferBusy(false);
    }
  }, [importBitwardenZip, setPendingImport, setTransferBusy, setTransferError, startedFor]);

  /// Content a KeePass import encrypted for entries that will not be stored.
  const dropBlobs = useCallback((blobIds: string[]) => {
    for (const blobId of blobIds) {
      void invoke("password_delete_attachment", { blobId }).catch(() => {});
    }
  }, []);

  /// The database opened: its entries go to the same filing question as any
  /// import. Its attachments are already in the silo, encrypted.
  const openKdbx = useCallback(
    async (password: string, keyFile: string | null) => {
      if (!kdbx) return;
      setTransferBusy(true);
      setKdbxError(null);
      const current = startedFor();
      try {
        if (kdbx.format === "silentsilo") {
          const read = await invoke<{
            entries: PasswordEntry[];
            categories: PasswordCategory[] | null;
            blobs: string[];
          }>("passwords_read_silentsilo", { path: kdbx.path, password });
          if (!current()) {
            dropBlobs(read.blobs);
            return;
          }
          setKdbx(null);
          setPendingImport({
            imported: withFreshIds(read.entries, entries),
            skipped: 0,
            source: "SilentSilo",
            sourceLabel: "SilentSilo",
            kind: "entry",
            extras: noExtras(),
            blobs: read.blobs,
            categories: read.categories ?? undefined,
          });
          return;
        }
        const read = await invoke<KdbxEntry[]>("passwords_read_kdbx", {
          path: kdbx.path,
          password: password || null,
          keyFile,
        });
        if (!current()) return;
        const { entries: imported, skipped } = kdbxToEntries(read, loadHistoryPolicy());
        dropBlobs(attachmentBlobs(skipped));
        setKdbx(null);
        setPendingImport({
          imported,
          skipped: skipped.length,
          source: "KeePass",
          sourceLabel: "KeePass",
          kind: "entry",
          extras: noExtras(),
          blobs: attachmentBlobs(imported),
        });
      } catch (e) {
        setKdbxError(formatAppError(e));
      } finally {
        setTransferBusy(false);
      }
    },
    [dropBlobs, entries, kdbx, setPendingImport, setTransferBusy, startedFor]
  );

  const cancelImport = useCallback(() => {
    if (pendingImport?.blobs) dropBlobs(pendingImport.blobs);
    setPendingImport(null);
  }, [dropBlobs, pendingImport, setPendingImport]);

  const finishImport = useCallback(
    (choice: ImportCategoryChoice) => {
      if (!pendingImport) return;
      const { skipped, source, sourceLabel, kind, extras } = pendingImport;
      const texts = IMPORT_TEXTS[kind];
      const imported = applyImportCategory(pendingImport.imported, choice);
      setPendingImport(null);

      // Appended, never merged over what's already there. An import that
      // silently overwrote existing entries would be unrecoverable, so an
      // entry whose content changed since the last export still arrives as a
      // second copy; only an exact match is dropped as a duplicate.
      const { fresh, duplicates } = dropDuplicates(entries, imported);
      if (fresh.length > 0) onImportEntries(fresh, source);
      // A SilentSilo export brings its categories' colours: any the silo
      // does not have yet are added, none it has are changed.
      const brought = pendingImport.categories ?? [];
      const known = new Set(storedCategories?.map((c) => c.name) ?? []);
      const added = brought.filter((c) => c.name && !known.has(c.name));
      if (fresh.length > 0 && added.length > 0) {
        onSaveCategories([...(storedCategories ?? []), ...added]);
      }
      if (pendingImport.blobs) {
        const kept = new Set(attachmentBlobs(fresh));
        dropBlobs(pendingImport.blobs.filter((id) => !kept.has(id)));
      }

      const skips: string[] = [];
      if (duplicates > 0) skips.push(t("pw.skip_duplicates", { count: duplicates }));
      if (skipped > 0) skips.push(t(texts.skipped, { count: skipped }));

      setTransferNotice(
        fresh.length === 0 && duplicates > 0
          ? t(texts.nothingNew, { count: duplicates })
          : [
              skips.length > 0
                ? t(texts.importedWith, {
                    count: fresh.length,
                    source: sourceLabel,
                    skips: skips.join(", "),
                  })
                : t(texts.imported, { count: fresh.length, source: sourceLabel }),
              describeExtras(extras),
            ]
              .filter(Boolean)
              .join(" ")
      );
    },
    [
      dropBlobs,
      entries,
      onImportEntries,
      onSaveCategories,
      pendingImport,
      setPendingImport,
      setTransferNotice,
      storedCategories,
    ]
  );

  /// Every export asks for the key, each time, whatever was touched a
  /// moment ago for one entry: a file of every password is a broader thing
  /// to hand over. The backend asks too when it was not asked here; here it
  /// is asked before the file is chosen, so somebody who cannot produce the
  /// key is not first made to pick where a file that will not be written
  /// goes. A silo with no key has nothing to ask.
  const confirmExport = useCallback(async (): Promise<boolean> => {
    setTransferError(null);
    setVerifying(true);
    try {
      await invoke("fido_reverify", { purpose: "export" });
      return true;
    } catch (e) {
      setTransferError(formatAppError(e));
      return false;
    } finally {
      setVerifying(false);
    }
  }, [setTransferError]);

  /// Every entry, every kind, encrypted: the file KeePassXC and KeePassDX
  /// open. Protected entries go as the window holds them; the backend puts
  /// in what it held back.
  const startKdbxExport = useCallback(async () => {
    setConfirmingExport(false);
    if (!(await confirmExport())) return;
    setKdbxError(null);
    setKdbx({ mode: "export", path: "", format: "kdbx" });
  }, [confirmExport]);

  /// Every entry as the silo keeps it, for another silo or another person
  /// who uses SilentSilo: kinds, settings, history and files come back
  /// exactly, which a KeePass file cannot promise.
  const startSilentsiloExport = useCallback(async () => {
    setConfirmingExport(false);
    if (!(await confirmExport())) return;
    setKdbxError(null);
    setKdbx({ mode: "export", path: "", format: "silentsilo" });
  }, [confirmExport]);

  const finishKdbxExport = useCallback(
    async (password: string) => {
      setKdbxError(null);
      const own = kdbx?.format === "silentsilo";
      try {
        const path = await saveFileDialog(
          own
            ? {
                defaultPath: `Passwords.${SILENTSILO_EXPORT}`,
                filters: [{ name: t("pw.sse_filter"), extensions: [SILENTSILO_EXPORT] }],
              }
            : {
                defaultPath: "silentsilo-passwords.kdbx",
                filters: [{ name: t("pw.kdbx_filter"), extensions: ["kdbx"] }],
              },
        );
        if (!path) return;
        setTransferBusy(true);
        if (own) {
          await invoke("passwords_write_silentsilo", {
            path,
            password,
            entries: JSON.stringify(toExport),
            categories: storedCategories ? JSON.stringify(storedCategories) : null,
          });
        } else {
          await invoke("passwords_write_kdbx", { path, password, entries: JSON.stringify(toExport) });
        }
        setKdbx(null);
        setExportOnly(null);
        setTransferNotice(
          own
            ? t("pw.sse_exported", { count: toExport.length })
            : t("pw.kdbx_exported", { count: toExport.length }),
        );
      } catch (e) {
        setKdbxError(formatAppError(e));
      } finally {
        setTransferBusy(false);
      }
    },
    [kdbx, storedCategories, toExport, setTransferBusy, setTransferNotice]
  );

  const handleExport = useCallback(async () => {
    setTransferError(null);

    // The CSV dialect other managers read has columns for logins only.
    const logins = toExport.filter((e) => typeOf(e) === "login");

    // An export writes every one of these into a plaintext file, which is
    // the broadest reveal in the app, so it asks for the key every time.
    if (!(await confirmExport())) return;

    const path = await saveFileDialog({
      defaultPath: "silentsilo-passwords.csv",
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (!path) return;

    setTransferBusy(true);
    try {
      // Protected entries come whole only now, for the confirmed export.
      const withheld = logins.filter((e) => e.withheld).map((e) => e.id);
      const whole = new Map<string, PasswordEntry>();
      if (withheld.length > 0) {
        const json = await invoke<string>("passwords_for_export", { ids: withheld });
        for (const entry of JSON.parse(json) as PasswordEntry[]) {
          whole.set(entry.id, withBaseFields(entry));
        }
      }
      await invoke("passwords_write_export_csv", {
        path,
        contents: entriesToCsv(logins.map((e) => whole.get(e.id) ?? e)),
        count: logins.length,
      });
      const leftOut = toExport.length - logins.length;
      setTransferNotice(
        [
          t("pw.csv_exported", { count: logins.length }),
          leftOut > 0 ? t("pw.csv_left_out", { count: leftOut }) : "",
          t("pw.export_csv_formula"),
        ]
          .filter(Boolean)
          .join(" ")
      );
    } catch (e) {
      setTransferError(formatAppError(e));
    } finally {
      setTransferBusy(false);
      setConfirmingExport(false);
      setExportOnly(null);
    }
  }, [confirmExport, toExport, setTransferBusy, setTransferError, setTransferNotice]);

  return (
    <div className="pw-view">
      <ViewHeader icon={KeyRound} title={t("nav.passwords")} subtitle={headerSummary} />
      {/* Toolbar spans all three panes: search and transfer act on the whole
          store, not on any one pane. */}
      <div className="view-toolbar">
        <div className="view-search">
          <span className="search-icon">
            <IconSearch size={16} />
          </span>
          <input
            type="text"
            placeholder={t("pw.search")}
            aria-label={t("pw.search_label")}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <button
          type="button"
          className={`pw-favicon-toggle${showFavicons ? " active" : ""}`}
          onClick={toggleShowFavicons}
          data-tooltip={
            showFavicons ? t("pw.site_icons_on_tip") : t("pw.site_icons_off_tip")
          }
        >
          {showFavicons ? <IconEye size={15} /> : <IconEyeOff size={15} />}
          <span>{t("pw.site_icons")}</span>
        </button>
        <button
          type="button"
          className="pw-transfer-btn"
          disabled={busy}
          onClick={() => setTrashOpen(true)}
          data-tooltip={t("pw.trash_tip")}
        >
          <Trash2 size={15} />
          <span>{t("pw.trash_button")}</span>
        </button>
        <button
          type="button"
          className="pw-transfer-btn"
          disabled={busy || transferBusy}
          onClick={() => void handleImport()}
          data-tooltip={t("pw.import_tip")}
        >
          <Upload size={15} />
          <span>{t("pw.import")}</span>
        </button>
        <button
          type="button"
          className="pw-transfer-btn"
          disabled={busy || transferBusy || entries.length === 0}
          onClick={() => {
            setTransferError(null);
            setTransferNotice(null);
            setExportOnly(null);
            setConfirmingExport(true);
          }}
          data-tooltip={t("pw.export_tip")}
        >
          <Download size={15} />
          <span>{t("pw.export_all")}</span>
        </button>
        <div className="pw-add-wrap">
          <button
            type="button"
            className="btn-primary btn-add-password"
            disabled={busy}
            aria-expanded={addMenuOpen}
            onClick={() => setAddMenuOpen((v) => !v)}
          >
            <IconPlus size={16} />
            <span>{t("pw.add_entry")}</span>
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
          what={t(IMPORT_TEXTS[pendingImport.kind].what, {
            count: pendingImport.imported.length,
            source: pendingImport.sourceLabel,
          })}
          categories={categories.map((c) => c.name)}
          onConfirm={finishImport}
          onCancel={cancelImport}
        />
      )}

      {kdbx && (
        <KdbxPasswordDialog
          mode={kdbx.mode}
          format={kdbx.format}
          busy={transferBusy}
          error={kdbxError}
          onSubmit={(password, keyFile) =>
            void (kdbx.mode === "open" ? openKdbx(password, keyFile) : finishKdbxExport(password))
          }
          onCancel={() => {
            setKdbx(null);
            if (kdbx.mode === "export") setExportOnly(null);
          }}
        />
      )}

      {confirmingExport && (
        <ExportChoiceDialog
          selected={exportOnly ? exportOnly.length : null}
          busy={transferBusy}
          onChoose={(kind) => {
            setConfirmingExport(false);
            void (kind === "silentsilo"
              ? startSilentsiloExport()
              : kind === "kdbx"
                ? startKdbxExport()
                : handleExport());
          }}
          onCancel={() => {
            setConfirmingExport(false);
            setExportOnly(null);
          }}
        />
      )}

      {transferNotice && (
        <div className="pw-transfer-notice" role="status">
          <span>{transferNotice}</span>
          <button type="button" className="link" onClick={() => setTransferNotice(null)}>
            {t("pw.dismiss")}
          </button>
        </div>
      )}

      {transferError && (
        <div className="pw-transfer-notice is-error" role="alert">
          <span>{transferError}</span>
          <button type="button" className="link" onClick={() => setTransferError(null)}>
            {t("pw.dismiss")}
          </button>
        </div>
      )}

      {verifying && (
        <div className="pw-transfer-notice" role="status">
          <span>
            {fidoProgress ?? t("pw.touch_key")}
          </span>
        </div>
      )}

      {editLockNoticeUp && (
        <div className="pw-transfer-notice" role="status">
          <span>{t("pw.edit_lock")}</span>
        </div>
      )}

      {entries.length === 0 && !editing ? (
        <div className="empty-state">
          <ShieldCheck size={48} className="empty-icon" />
          <p className="empty-title">{t("pw.empty_title")}</p>
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
          <p className="hint">{t("pw.empty_hint")}</p>
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
                  <p className="empty-title">{t("pw.no_matches")}</p>
                  <p className="hint">{t("pw.no_matches_hint")}</p>
                </div>
              ) : selectedType ? (
                (() => {
                  const Icon = TYPE_ICONS[selectedType];
                  return (
                    <div className="empty-state">
                      <Icon size={36} className="empty-icon" />
                      <p className="empty-title">{t(TYPE_TEXTS[selectedType].noneYet)}</p>
                      <button
                        className="btn-primary"
                        type="button"
                        disabled={busy}
                        onClick={() => startCreate(selectedType)}
                      >
                        <IconPlus size={15} />
                        {t(TYPE_TEXTS[selectedType].add)}
                      </button>
                    </div>
                  );
                })()
              ) : (
                <div className="empty-state">
                  <ShieldCheck size={36} className="empty-icon" />
                  <p className="empty-title">{t("pw.category_empty")}</p>
                  <p className="hint">{t("pw.category_empty_hint")}</p>
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
                checked={checked}
                onCheck={checkEntry}
                onClearChecked={clearChecked}
              />
            )}
          </div>

          {/* The same bar Files shows for a selection, with what applies to
              logins: a category, a star, deletion. */}
          {checked.size > 0 && (
            <div className="selection-toolbar">
              <span className="selection-toolbar-count">
                {t("trash.selected", { count: checked.size })}
              </span>
              <div className="selection-toolbar-actions">
                <div className="pw-bulk-move">
                  <button
                    type="button"
                    className="selection-toolbar-btn"
                    disabled={busy}
                    aria-expanded={movingChecked}
                    onClick={() => setMovingChecked((open) => !open)}
                  >
                    <FolderInput size={14} /> {t("pw.bulk_move")}
                  </button>
                  {movingChecked && (
                    <div className="pw-bulk-menu" role="menu">
                      {categories.map((c) => (
                        <button
                          key={c.name}
                          type="button"
                          role="menuitem"
                          onClick={() => void moveChecked(c.name)}
                        >
                          <span
                            className="pw-bulk-dot"
                            style={{ background: colorFor(c.name) }}
                            aria-hidden
                          />
                          {c.name}
                        </button>
                      ))}
                    </div>
                  )}
                </div>
                <button
                  type="button"
                  className="selection-toolbar-btn"
                  disabled={busy}
                  onClick={() => void starChecked()}
                >
                  <Star size={14} /> {allStarred ? t("pw.bulk_unfavorite") : t("pw.bulk_favorite")}
                </button>
                <button
                  type="button"
                  className="selection-toolbar-btn"
                  disabled={busy || transferBusy}
                  onClick={() => {
                    setTransferError(null);
                    setTransferNotice(null);
                    setExportOnly(checkedEntries);
                    setConfirmingExport(true);
                  }}
                >
                  <Download size={14} /> {t("pw.export")}
                </button>
                <button
                  type="button"
                  className="selection-toolbar-btn danger"
                  disabled={busy}
                  onClick={() => setPendingBulkDelete(true)}
                >
                  <IconTrash size={14} /> {t("pw.delete")}
                </button>
                <span className="selection-toolbar-sep" aria-hidden />
                {checked.size < filtered.length && (
                  <button type="button" className="selection-toolbar-btn" onClick={checkAll}>
                    <ListChecks size={14} /> {t("trash.select_all")}
                  </button>
                )}
                <button type="button" className="selection-toolbar-btn" onClick={clearChecked}>
                  <IconClose size={14} /> {t("files.clear_selection")}
                </button>
              </div>
            </div>
          )}

          <div className="pw-detail-pane">
            {editing ? (
              <EntryEditor
                os={os}
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
                onCopySecretField={(entry, key, pick, field) =>
                  void copySecretField(entry, key, pick, field)
                }
                onOpenAttachment={(attachment) => void openAttachment(selected, attachment)}
                onRequestReveal={requestReveal}
                onToggleFavorite={(entry) =>
                  onSaveEntry(withEdits(entry, { favorite: !entry.favorite }), "arranged")
                }
                onEdit={(entry) => void startEdit(entry)}
                onDelete={() => setPendingDelete(selected)}
                onRestoreVersion={(entry, version) => void restoreVersion(entry, version)}
                onClearHistory={(entry) => setPendingClearHistory(entry)}
              />
            ) : filtered.length > 0 ? (
              // Something to do, not only something to read: adding the
              // kind in view, or bringing a list in from another manager.
              <div className="pw-detail-placeholder">
                <MousePointerClick size={32} className="empty-icon" />
                <p className="empty-title">{t("pw.select_hint")}</p>
                <div className="actions">
                  <button
                    type="button"
                    className="btn-primary"
                    disabled={busy}
                    onClick={() => startCreate(selectedType ?? "login")}
                  >
                    <IconPlus size={15} />
                    {t(TYPE_TEXTS[selectedType ?? "login"].add)}
                  </button>
                  <button
                    type="button"
                    className="btn-secondary"
                    disabled={busy || transferBusy}
                    onClick={() => void handleImport()}
                  >
                    <Upload size={15} />
                    {t("pw.import")}
                  </button>
                </div>
              </div>
            ) : // An empty list already says everything; a second pane
            // repeating "select something" would be advice about nothing.
            null}
          </div>
        </div>
      )}

      {pendingDelete && (
        <ConfirmDialog
          title={t("pw.delete_title")}
          message={[
            backedUp
              ? t("pw.delete_synced", { name: pendingDelete.service })
              : t("pw.delete_local", { name: pendingDelete.service }),
            t("pw.delete_to_trash"),
            backedUp && (pendingDelete.attachments ?? []).length > 0
              ? t("pw.delete_attachments_kept")
              : "",
            !backedUp || archiveTargets === 0
              ? ""
              : (pendingDelete.attachments ?? []).length > 0
                ? t("pw.delete_archive_with_files")
                : t("pw.delete_archive"),
          ]
            .filter(Boolean)
            .join(" ")}
          confirmLabel={t("pw.delete")}
          danger
          busy={busy}
          onConfirm={() => void confirmDelete()}
          onCancel={() => setPendingDelete(null)}
        />
      )}

      {pendingBulkDelete && (
        <ConfirmDialog
          title={t("pw.bulk_delete_title", { count: checkedEntries.length })}
          message={[
            backedUp ? t("pw.bulk_delete_synced") : t("pw.bulk_delete_local"),
            t("pw.delete_to_trash"),
            backedUp && checkedEntries.some((e) => (e.attachments ?? []).length > 0)
              ? t("pw.delete_attachments_kept")
              : "",
            !backedUp || archiveTargets === 0
              ? ""
              : checkedEntries.some((e) => (e.attachments ?? []).length > 0)
                ? t("pw.delete_archive_with_files")
                : t("pw.delete_archive"),
          ]
            .filter(Boolean)
            .join(" ")}
          confirmLabel={t("pw.bulk_delete_button", { count: checkedEntries.length })}
          danger
          busy={busy}
          onConfirm={() => void confirmBulkDelete()}
          onCancel={() => setPendingBulkDelete(false)}
        />
      )}

      {trashOpen && (
        <PasswordTrashDialog onRestore={onRestoreEntry} onClose={() => setTrashOpen(false)} />
      )}

      {pendingClearHistory && (
        <ConfirmDialog
          title={t("pw.clear_history_title")}
          message={[
            backedUp
              ? t("pw.clear_history_synced", { name: pendingClearHistory.service })
              : t("pw.clear_history_local", { name: pendingClearHistory.service }),
            t("pw.records_cleared_later"),
            backedUp && archiveTargets > 0 ? t("pw.clear_history_archive") : "",
          ]
            .filter(Boolean)
            .join(" ")}
          confirmLabel={t("pw.clear_history")}
          danger
          busy={busy}
          onConfirm={() => {
            void onSaveEntry(withoutHistory(pendingClearHistory), "history_cleared");
            setPendingClearHistory(null);
          }}
          onCancel={() => setPendingClearHistory(null)}
        />
      )}
    </div>
  );
}
