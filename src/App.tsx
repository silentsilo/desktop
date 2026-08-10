import type { MouseEvent } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { IconFilePlus } from "./ui/Icons";
import { join } from "@tauri-apps/api/path";
import { open, save } from "@tauri-apps/plugin-dialog";
import { useEventSubscription } from "./hooks/useEventSubscription";
import { useToasts } from "./hooks/useToasts";
import { useJobs } from "./hooks/useJobs";
import { mapPool, uploadConcurrency } from "./lib/pool";
import { breadcrumbSegments, formatBytes, formatStorage } from "./lib/format";
import type {
  Authenticator,
  BlobStatus,
  Bootstrap,
  DeviceInfo,
  RecoveryStatus,
  SearchHit,
  SpaceReport,
  Silo,
  FileEntry,
  FolderEntry,
  NavMode,
  PasswordCategory,
  PasswordEntry,
  SecurityKeyInfo,
  SiloIdleStatus,
  TrashItem,
  VaultEntry,
  VaultMeta,
  View,
} from "./lib/types";
import { AUTO_LOCK_OPTIONS_MINUTES } from "./lib/types";
import { shouldLock } from "./lib/autoLock";
import { securityKeyDisplayName } from "./lib/keyName";
import { runsOnOpen } from "./lib/executable";
import { checkForUpdate } from "./lib/updater";
import {
  shouldCheckForUpdate,
  UPDATE_POLL_INTERVAL_MS,
} from "./lib/updateSchedule";
import type { Update } from "@tauri-apps/plugin-updater";
import { useTheme } from "./lib/theme";
import { AppShell, type SyncIndicator } from "./layout/AppShell";
import { SiloPickerView } from "./views/SiloPickerView";
import { AuthShell } from "./layout/AuthShell";
import { ToastHost } from "./ui/ToastHost";
import { JoinView } from "./views/JoinView";
import { EnrollView } from "./views/EnrollView";
import { FilesExplorer } from "./views/FilesExplorer";
import { PasswordsPanel } from "./views/passwords/PasswordsPanel";
import { HealthPanel } from "./views/health/HealthPanel";
import { analyseHealth } from "./views/health/analysis";
import { FavoritesPanel } from "./views/FavoritesPanel";
import { CATEGORIES_ROW_ID, isCategoriesRow } from "./views/passwords/util";
import { SettingsPanel, type SettingsSectionId } from "./views/SettingsPanel";
import { BackupPanel } from "./views/BackupPanel";
import { CopiesPanel } from "./views/CopiesPanel";
import { VerifyPanel } from "./views/VerifyPanel";
import { ConfirmDialog } from "./views/ConfirmDialog";
import { ShellUploadDialog } from "./views/ShellUploadDialog";
import { ShellDownloadDialog } from "./views/ShellDownloadDialog";
import { TrashPanel } from "./views/TrashPanel";
import { UnlockView } from "./views/UnlockView";

const AUTO_LOCK_KEY = "silentsilo.autoLockMinutes";
const DEFAULT_AUTO_LOCK_MINUTES = 15;

const AUTO_UPDATE_KEY = "silentsilo.update.auto";
const UPDATE_LAST_CHECK_KEY = "silentsilo.update.lastCheckAt";
const UPDATE_NOTIFIED_KEY = "silentsilo.update.lastNotifiedVersion";

function loadAutoLockMinutes(): number {
  const saved = Number.parseInt(localStorage.getItem(AUTO_LOCK_KEY) ?? "", 10);
  return (AUTO_LOCK_OPTIONS_MINUTES as readonly number[]).includes(saved)
    ? saved
    : DEFAULT_AUTO_LOCK_MINUTES;
}

type ConfirmState = {
  /// Names the decision. "SilentSilo" as a heading told the user nothing
  /// they could not see from the window they were already looking at.
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
  option?: { label: string; hint?: string };
  resolve: (value: ConfirmResult) => void;
};

/// `option` is false whenever the dialog carried no checkbox, and always
/// false when the user cancelled: a ticked box on a dialog that was declined
/// is not permission for anything.
type ConfirmResult = { ok: boolean; option: boolean };

/// Remembers that the user declined the recovery-code step, so it is offered
/// once rather than at every unlock. The nudge in Settings stays regardless.

/// Added to both delete-for-good dialogs. Purging removes the current
/// content at once, but blobs left behind by earlier content replacements
/// are collected a sweep or two later, since a sweep only deletes what was
/// unreferenced on the previous pass too. Saying nothing would tell someone
/// their old draft is gone at a moment when it is not.
const SUPERSEDED_NOTE =
  " If a file's content was ever replaced, its earlier versions are cleared" +
  " by a later housekeeping pass rather than right now.";

/// Added when the silo has a copy the app never deletes from, where
/// "permanently" is false in the one place it has to be true: the entry
/// leaves the index and every ordinary copy, and the bytes stay on the
/// append-only one until its retention lets them go. Worth saying before
/// the click, not after.
/// The button on those dialogs, and the toast after them. "Permanently"
/// beside a message explaining that the bytes stay is the app
/// contradicting itself in one box, and the button is what people read.
function deleteForGoodLabel(archiveTargets: number): string {
  return archiveTargets > 0 ? "Delete for good" : "Delete permanently";
}

function archiveNote(archiveTargets: number): string {
  if (archiveTargets === 0) return "";
  return archiveTargets === 1
    ? " One of your copies never deletes anything, so the content stays there" +
        " until that storage's own rules remove it."
    : ` ${archiveTargets} of your copies never delete anything, so the content` +
        " stays there until that storage's own rules remove it.";
}

export default function App() {
  const toasts = useToasts();
  const [confirmDialog, setConfirmDialog] = useState<ConfirmState | null>(null);
  /// Copies the app never deletes from, so the delete-for-good dialogs can
  /// say what actually happens instead of promising "permanently".
  const [archiveTargets, setArchiveTargets] = useState(0);
  const askConfirmWith = useCallback(
    (
      title: string,
      message: string,
      opts?: {
        confirmLabel?: string;
        danger?: boolean;
        option?: { label: string; hint?: string };
      },
    ) =>
      new Promise<ConfirmResult>((resolve) => {
        setConfirmDialog({ title, message, resolve, ...opts });
      }),
    [],
  );
  /// Most questions are just yes or no.
  const askConfirm = useCallback(
    (
      title: string,
      message: string,
      opts?: { confirmLabel?: string; danger?: boolean },
    ) => askConfirmWith(title, message, opts).then((r) => r.ok),
    [askConfirmWith],
  );
  const [bootstrap, setBootstrap] = useState<Bootstrap | null>(null);
  const [meta, setMeta] = useState<VaultMeta | null>(null);
  const [currentFolder, setCurrentFolder] = useState<FolderEntry | null>(null);
  const [entries, setEntries] = useState<VaultEntry[]>([]);
  const [trashEntries, setTrashEntries] = useState<TrashItem[]>([]);
  const [newFolderName, setNewFolderName] = useState("");
  // One flag for the whole app used to mean that stepping into a folder
  // greyed out every button on screen. See useJobs.
  const { begin, end, busy } = useJobs();
  const [joining, setJoining] = useState(false);
  const [silos, setSilos] = useState<Silo[]>([]);
  const [silosLoaded, setSilosLoaded] = useState(false);
  const [dropActive, setDropActive] = useState(false);
  const [sync, setSync] = useState<SyncIndicator>({
    configured: false,
    pending: 0,
    state: "idle",
    lastSyncAt: null,
    lastError: null,
  });
  /// Which blobs are on this disk and which still owe the backup an upload,
  /// plus what that occupies. Null until the first read, which is what tells
  /// the explorer to show no badges rather than wrong ones.
  const [blobStatus, setBlobStatus] = useState<BlobStatus | null>(null);
  /// A download-everything pass in flight, counted in blobs. Null when none
  /// is running.
  const [contentFetch, setContentFetch] = useState<{ done: number; total: number } | null>(null);
  /// Which Settings section is open, held here so the sidebar's storage
  /// figure can send the user straight to Backup.
  const [settingsSection, setSettingsSection] = useState<SettingsSectionId>("auto-lock");
  const [globalResults, setGlobalResults] = useState<SearchHit[] | null>(null);
  const [searching, setSearching] = useState(false);
  // Whether the post-unlock recovery step is on screen. Only ever shown when
  // the silo has no code and the user hasn't already declined.
  const [uploadProgress, setUploadProgress] = useState<string | null>(null);
  const [uploadCancelable, setUploadCancelable] = useState(false);
  const [uploadCancelling, setUploadCancelling] = useState(false);
  const uploadCancelRef = useRef(false);
  const contentFetchCancelRef = useRef(false);
  // Held by ThemeProvider above this component, so the screens shown before
  // a silo opens — each of which returns early below — reach it too.
  const themeControl = useTheme();
  const theme = themeControl?.theme ?? "dark";

  const [autoUpdateEnabled, setAutoUpdateEnabledState] = useState(
    () => localStorage.getItem(AUTO_UPDATE_KEY) !== "off",
  );
  const setAutoUpdateEnabled = (on: boolean) => {
    setAutoUpdateEnabledState(on);
    localStorage.setItem(AUTO_UPDATE_KEY, on ? "on" : "off");
  };
  /// An update the scheduled check found, held so Settings can offer the
  /// install without asking the endpoint a second time.
  const [backgroundUpdate, setBackgroundUpdate] = useState<{
    version: string;
    update: Update;
  } | null>(null);

  /// The fallback a silo without its own timeout follows. Read here and
  /// shown as a label in Settings, which belongs to one silo and so has no
  /// business setting a value for the others.
  const [autoLockMinutes] = useState(loadAutoLockMinutes);
  /// The focused silo's own timeout. Null means it follows the default,
  /// which is a different statement from "never".
  const [siloAutoLockMinutes, setSiloAutoLockMinutes] = useState<number | null>(null);


  const [view, setView] = useState<View>("files");
  const [fidoProgress, setFidoProgress] = useState<string | null>(null);
  const [navHistory, setNavHistory] = useState<string[]>([]);
  const [navIndex, setNavIndex] = useState(0);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [anchorIndex, setAnchorIndex] = useState<number | null>(null);
  const [renamingId, setRenamingId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [securityKeys, setSecurityKeys] = useState<SecurityKeyInfo[]>([]);
  const [recovery, setRecovery] = useState<RecoveryStatus>({ enabled: false, created_at: null });
  // Held in memory only, and only until the user says they've written it
  // down: nothing anywhere else can produce it again.
  const [recoveryCode, setRecoveryCode] = useState<string | null>(null);
  const [newKeyLabel, setNewKeyLabel] = useState("");
  const [keyAddSuccess, setKeyAddSuccess] = useState<string | null>(null);
  const [passwordEntries, setPasswordEntries] = useState<PasswordEntry[]>([]);
  /// The stored category list, or null when the silo never saved one; the
  /// panel derives a starting list from the entries in that case.
  const [passwordCategories, setPasswordCategories] = useState<PasswordCategory[] | null>(null);
  const [passwordsLoaded, setPasswordsLoaded] = useState(false);
  /// Whether the key list and the recovery status have been read for this
  /// silo. Health must not judge a silo it has not finished reading.
  const [siloFactsLoaded, setSiloFactsLoaded] = useState(false);
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  /// Starred files and folders. Held here rather than inside the Favourites
  /// panel because starring happens in the explorer, which has to be able to
  /// keep the list current.
  const [favoriteHits, setFavoriteHits] = useState<SearchHit[]>([]);
  /// The entry Health sent the user to. Cleared on leaving Credentials, so
  /// the view does not jump back to it later, and so returning to Health and
  /// picking the same entry again is a change the panel notices.
  const [focusEntryId, setFocusEntryId] = useState<string | null>(null);
  const [pendingShellUploadPaths, setPendingShellUploadPaths] = useState<string[] | null>(null);
  const [shellUploadBusy, setShellUploadBusy] = useState(false);
  const [pendingShellDownloadTarget, setPendingShellDownloadTarget] = useState<string | null>(null);
  const [shellDownloadBusy, setShellDownloadBusy] = useState(false);
  const unlockedRef = useRef(false);
  const viewRef = useRef<View>("files");
  /// Read inside the auto-lock sweep, which must not restart every time
  /// bootstrap changes — a re-created interval would keep pushing the next
  /// sweep further out.
  const focusedSiloRef = useRef<string | null>(null);
  const importPathsRef = useRef<
    ((paths: string[], verb: "Pasted" | "Added") => Promise<void>) | null
  >(null);

  const refreshBootstrap = useCallback(async () => {
    const b = await invoke<Bootstrap>("app_bootstrap");
    setBootstrap(b);
  }, []);

  const checkPendingShellUploads = useCallback(async () => {
    try {
      const paths = await invoke<string[]>("shell_upload_queue_pending");
      if (paths.length === 0) return;
      setPendingShellUploadPaths(paths);
    } catch {
      // Silo isn't actually unlocked yet — the queue stays on disk and this
      // gets retried right after the next successful unlock.
    }
  }, []);

  const checkPendingShellDownloads = useCallback(async () => {
    try {
      const targetDir = await invoke<string | null>("shell_download_queue_pending");
      if (!targetDir) return;
      setPendingShellDownloadTarget(targetDir);
    } catch {
      // Silo isn't actually unlocked yet — the queue stays on disk and this
      // gets retried right after the next successful unlock.
    }
  }, []);

  useEventSubscription(
    () =>
      listen<string>("fido-progress", (event) => {
        setFidoProgress(event.payload);
      }),
    [],
  );

  // Rust locks every silo when the workstation locks or suspends. The screen
  // behind that has to follow, or the user comes back to an explorer full of
  // file names belonging to a silo that is no longer open.
  useEventSubscription(
    () =>
      listen("silos-locked", () => {
        resetExplorer();
        void refreshBootstrap();
      }),
    [],
  );

  // Suppress the native WebView right-click menu everywhere by default —
  // only the file explorer wires up a real context menu (folder/file
  // actions); other screens have no use for a right-click menu at all.
  useEffect(() => {
    const handler = (e: globalThis.MouseEvent) => e.preventDefault();
    document.addEventListener("contextmenu", handler);
    return () => document.removeEventListener("contextmenu", handler);
  }, []);

  useEventSubscription(
    () =>
      listen("shell-upload-pending", () => {
        // Only safe to drain the queue once we know a session is open —
        // otherwise vault_list_all_folders would fail and the paths would be
        // lost rather than left for the post-unlock check to pick up.
        if (unlockedRef.current) {
          void checkPendingShellUploads();
        }
      }),
    [checkPendingShellUploads],
  );

  useEventSubscription(
    () =>
      listen("shell-download-pending", () => {
        if (unlockedRef.current) {
          void checkPendingShellDownloads();
        }
      }),
    [checkPendingShellDownloads],
  );

  // Dropping files on the window is the first thing anyone tries in a file
  // manager. Registered once and reading through a ref, because re-binding
  // the OS-level handler on every navigation would drop events mid-drag.
  useEventSubscription(
    () =>
      getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === "over") {
          setDropActive(unlockedRef.current && viewRef.current === "files");
          return;
        }
        if (event.payload.type === "leave") {
          setDropActive(false);
          return;
        }
        setDropActive(false);
        // Only the file browser has somewhere to put them; dropping onto
        // Settings or Passwords should do nothing rather than something
        // surprising.
        if (!unlockedRef.current || viewRef.current !== "files") return;
        void importPathsRef.current?.(event.payload.paths, "Added");
      }),
    [],
  );

  // Changes made on another device arrive between renders, so the list on
  // screen is stale the moment a pass applies anything.
  useEventSubscription(
    () =>
      listen("vault-changed", () => {
        if (unlockedRef.current) {
          void refreshCurrentFolderRef.current?.();
        }
      }),
    [],
  );

  // A rename forced by another device's changes is the one thing here worth
  // interrupting for: a file the user knows by name is now called something
  // else, and nothing else on screen would say so.
  useEventSubscription(
    () =>
      listen<{ renamed: string[]; needs_rebuild: boolean }>("sync-report", (event) => {
        if (event.payload.needs_rebuild) {
          setNeedsRebuild(true);
          return;
        }
        setSync((prev) => ({ ...prev, state: "ok", lastSyncAt: Date.now(), lastError: null }));
        for (const rename of event.payload.renamed) {
          toasts.info(`Renamed to avoid a clash on another device: ${rename}`);
        }
      }),
    [toasts],
  );

  /// This device was away long enough that the changes it is missing have
  /// been compacted out of the silo. Asked as a dialog rather than a toast:
  /// nothing syncs until it is answered, in either direction.
  const [needsRebuild, setNeedsRebuild] = useState(false);
  const [rebuilding, setRebuilding] = useState(false);

  const rebuildFromSnapshot = async () => {
    setRebuilding(true);
    try {
      await invoke("vault_rebuild_from_snapshot");
      setNeedsRebuild(false);
      await refreshCurrentFolder();
      await refreshSync();
      toasts.success("This device is back in step with the silo.");
    } catch (e) {
      toasts.error(e);
    } finally {
      setRebuilding(false);
    }
  };

  useEffect(() => {
    importPathsRef.current = importPaths;
  });

  useEffect(() => {
    viewRef.current = view;
  }, [view]);

  /**
   * Recovers an explorer that is open on nothing. Every action reads the
   * current folder and returns quietly if there isn't one, so that state
   * looks normal and does nothing at all: no error, no clue. Making it
   * self-correcting beats a dead window the user can only escape by
   * restarting.
   */
  useEffect(() => {
    if (!meta || currentFolder) return;
    void (async () => {
      try {
        const root = await invoke<FolderEntry>("vault_root_folder");
        setCurrentFolder(root);
        setEntries(await invoke<VaultEntry[]>("vault_list_folder", { folderId: root.id }));
        setNavHistory([root.id]);
        setNavIndex(0);
      } catch {
        // Locked or mid-teardown; the screens that own those states handle it.
      }
    })();
  }, [meta, currentFolder]);

  useEffect(() => {
    focusedSiloRef.current = bootstrap?.silo?.id ?? null;
    void refreshSiloAutoLock(bootstrap?.silo?.id ?? null);
    unlockedRef.current = Boolean(
      bootstrap?.provisioned && !bootstrap.locked && meta !== null && bootstrap.fido_enrolled,
    );
    // refreshSiloAutoLock is a useCallback([]) declared further down, so it
    // never changes identity; naming it here would read it before its
    // declaration and throw.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bootstrap, meta]);

  const clearSelection = useCallback(() => {
    setSelectedIds(new Set());
    setAnchorIndex(null);
    setRenamingId(null);
  }, []);

  const resetExplorer = useCallback(() => {
    setMeta(null);
    setEntries([]);
    setCurrentFolder(null);
    setNavHistory([]);
    setNavIndex(0);
    clearSelection();
    setSecurityKeys([]);
  }, [clearSelection]);

  /// Whether this device is meant to hold every blob rather than an index of
  /// them. Read with the rest of the silo's facts and after any change, since
  /// it decides whether "this computer is one of my copies" is true.
  const [fullCopy, setFullCopy] = useState(false);
  const refreshFullCopy = useCallback(async () => {
    try {
      const status = await invoke<{ enabled: boolean }>("full_copy_status");
      setFullCopy(status.enabled);
    } catch {
      setFullCopy(false);
    }
  }, []);

  const setFullCopyEnabled = async (on: boolean) => {
    try {
      await invoke("set_full_copy", { enabled: on });
      setFullCopy(on);
      if (on) {
        // Fetching happens on the sync pass, so say what will happen rather
        // than letting a checkbox tick and nothing visibly follow.
        toasts.info("This computer will fetch anything it is missing in the background.");
        await refreshSync();
      }
    } catch (e) {
      toasts.error(e);
      await refreshFullCopy();
    }
  };

  /// Room left where the silo lives. Read when the silo opens and after an
  /// import, the only moments it changes by anything worth saying.
  /// Bought space, as the service last counted it. Null whenever the answer
  /// could not be had, and Health then says nothing rather than guessing.
  const [hostedUsage, setHostedUsage] = useState<{
    account_bytes: number;
    quota_bytes: number;
    status?: string;
  } | null>(null);

  const [diskSpace, setDiskSpace] = useState<SpaceReport | null>(null);
  const refreshDiskSpace = useCallback(async () => {
    try {
      setDiskSpace(await invoke<SpaceReport>("vault_disk_space", { paths: [] }));
    } catch {
      // An unplugged drive answers this way, and it is not news: the silo
      // being unreachable is already visible everywhere else.
      setDiskSpace(null);
    }
  }, []);

  const loadRootExplorer = useCallback(async (recoveryCodeUsed?: string) => {
    // Same session either way — a code and a key both end up handing the
    // silo its data encryption key, and nothing downstream can tell which.
    const m = recoveryCodeUsed
      ? await invoke<VaultMeta>("vault_unlock_with_recovery", { code: recoveryCodeUsed })
      : await invoke<VaultMeta>("vault_unlock");
    setMeta(m);
    const root = await invoke<FolderEntry>("vault_root_folder");
    setCurrentFolder(root);
    const list = await invoke<VaultEntry[]>("vault_list_folder", { folderId: root.id });
    setEntries(list);
    setNavHistory([root.id]);
    setNavIndex(0);
    clearSelection();

    // Protected folders are scanned here rather than by a watcher, because
    // this is the moment the vault is open and the user is present. Not
    // awaited: a folder with thousands of files would otherwise hold the
    // explorer closed while it worked, and nothing on screen depends on it.
    void refreshDiskSpace();
    void refreshFullCopy();
    void invoke<{ imported: number }>("protected_folders_scan")
      .then((report) => {
        if (report.imported > 0) void refreshCurrentFolderRef.current?.();
      })
      .catch(() => {
        // A folder on an unplugged drive is the ordinary case, not an error
        // worth interrupting someone who just opened their silo.
      });
  }, [clearSelection, refreshDiskSpace, refreshFullCopy]);

  const retryFidoDetection = async () => {
    begin("keys");
    try {
      await refreshBootstrap();
    } catch (e) {
      toasts.error(e);
    } finally {
      end("keys");
    }
  };

  const refreshTrash = useCallback(async () => {
    try {
      const list = await invoke<TrashItem[]>("vault_list_trash");
      setTrashEntries(list);
    } catch {
      // Silent — this also runs opportunistically right after unlock (to
      // populate the sidebar badge) and on every trash/restore action, so a
      // toast here would be noisy; the Trash view's own visit still surfaces
      // failures loudly enough to matter.
    }
  }, []);

  useEffect(() => {
    void refreshBootstrap();
  }, [refreshBootstrap]);

  /**
   * A silo the backend still holds open is stepped into, not asked about:
   * on launch, bootstrap reported the session as unlocked while `meta` was
   * still null, so the key prompt appeared for a silo that needed no key.
   * Guarded on `meta === null` so it fires only outside a silo; every other
   * bootstrap refresh happens with meta set, and re-running would throw the
   * user back to the root folder.
   */
  /// A key change left half done makes every sync pass fail against storage
  /// that is part-way converted, and the only way out is forward. Keyed on
  /// the silo being open rather than on the unlock command, because joining,
  /// recovery and stepping into a silo left unlocked all arrive here too.
  const rotationCheckedRef = useRef<string | null>(null);
  useEffect(() => {
    const siloId = bootstrap?.silo?.id ?? null;
    if (!siloId || bootstrap?.locked) {
      rotationCheckedRef.current = null;
      return;
    }
    if (rotationCheckedRef.current === siloId) return;
    rotationCheckedRef.current = siloId;
    void (async () => {
      try {
        const pending = await invoke<boolean>("vault_rotation_pending");
        setRotationPending(pending);
        if (pending) {
          setView("settings");
          setSettingsSection("keys");
          toasts.info(
            "A key change on this silo was never finished. Syncing will fail until it is.",
          );
        }
      } catch {
        setRotationPending(false);
      }
    })();
  }, [bootstrap?.silo?.id, bootstrap?.locked, toasts]);

  /// Bought space, for the meter in Backup and the Health finding about
  /// running out. Keyed on the silo being open rather than on the unlock,
  /// because resuming into one already unlocked and stepping into another
  /// both arrive here without passing through that path. Never awaited and
  /// never surfaced: a figure allowed to be missing must not hold up a silo.
  useEffect(() => {
    const siloId = bootstrap?.silo?.id ?? null;
    if (!siloId || bootstrap?.locked) {
      setHostedUsage(null);
      return;
    }
    let current = true;
    void invoke<{ account_bytes: number; quota_bytes: number; status?: string } | null>(
      "hosted_usage",
    )
      .then((u) => {
        if (current) setHostedUsage(u);
      })
      .catch(() => {
        if (current) setHostedUsage(null);
      });
    return () => {
      current = false;
    };
  }, [bootstrap?.silo?.id, bootstrap?.locked]);

  const steppingInRef = useRef(false);
  useEffect(() => {
    if (steppingInRef.current) return;
    if (!bootstrap?.silo || bootstrap.locked || !bootstrap.fido_enrolled) return;
    if (meta !== null) return;
    steppingInRef.current = true;
    void (async () => {
      try {
        await finishOpeningSilo(await invoke<VaultMeta>("vault_meta"));
      } catch {
        // The session went away between the two calls; the key prompt is
        // then the right screen after all.
      } finally {
        steppingInRef.current = false;
      }
    })();
  });

  useEffect(() => {
    if (view === "trash" && meta) {
      void refreshTrash();
    }
  }, [view, meta, refreshTrash]);

  const refreshSilos = useCallback(async () => {
    try {
      setSilos(await invoke<Silo[]>("silo_list"));
    } catch {
      setSilos([]);
    } finally {
      setSilosLoaded(true);
    }
  }, []);

  /// Reads the focused silo's own timeout out of the idle report, which is
  /// where the backend already publishes it.
  const refreshSiloAutoLock = useCallback(async (siloId: string | null) => {
    if (!siloId) {
      setSiloAutoLockMinutes(null);
      return;
    }
    try {
      const idle = await invoke<SiloIdleStatus[]>("silo_idle_status");
      setSiloAutoLockMinutes(idle.find((s) => s.id === siloId)?.auto_lock_minutes ?? null);
    } catch {
      setSiloAutoLockMinutes(null);
    }
  }, []);

  const setSiloAutoLock = async (minutes: number | null) => {
    const id = focusedSiloRef.current;
    if (!id) return;
    try {
      await invoke("silo_set_auto_lock", { id, minutes });
      setSiloAutoLockMinutes(minutes);
    } catch (e) {
      toasts.error(e);
    }
  };

  // The registry is read once at launch, alongside the bootstrap: which
  // silos exist and which one is open are two halves of the same answer.
  useEffect(() => {
    void refreshSilos();
  }, [refreshSilos]);

  /**
   * Scheduled update check: at most one successful request per day,
   * evaluated hourly, because the app is left running for days and a
   * launch-only check would go quiet on the machines that need patches
   * most. The timestamp is written only after a completed request, so an
   * offline laptop does not lose its daily window to a failed attempt.
   * Failures are silent; the manual button in Settings bypasses this.
   */
  useEffect(() => {
    if (!autoUpdateEnabled) return;
    let cancelled = false;

    const runIfDue = async () => {
      const raw = localStorage.getItem(UPDATE_LAST_CHECK_KEY);
      const last = raw === null ? null : Number.parseInt(raw, 10);
      if (!shouldCheckForUpdate(last, Date.now())) return;
      try {
        const result = await checkForUpdate();
        localStorage.setItem(UPDATE_LAST_CHECK_KEY, String(Date.now()));
        if (cancelled || !result.available) return;
        setBackgroundUpdate({ version: result.version, update: result.update });
        // Said once per version, not once per day: the same toast every
        // morning trains people to dismiss it unread.
        if (localStorage.getItem(UPDATE_NOTIFIED_KEY) !== result.version) {
          localStorage.setItem(UPDATE_NOTIFIED_KEY, result.version);
          toasts.info(`SilentSilo ${result.version} is available. Install it from Settings.`);
        }
      } catch {
        // Offline or endpoint unreachable. The next hourly pass retries.
      }
    };

    void runIfDue();
    const timer = window.setInterval(() => void runIfDue(), UPDATE_POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [autoUpdateEnabled, toasts]);

  const createSilo = async (name: string, location: string | null) => {
    begin("silo");
    try {
      await invoke("silo_create", { name, location });
      await refreshSilos();
      setBootstrap(await invoke<Bootstrap>("app_bootstrap"));
      resetExplorer();
      toasts.success(`Silo “${name}” created. Enrol a key to lock it.`);
    } catch (e) {
      toasts.error(e);
    } finally {
      end("silo");
    }
  };

  const renameSilo = async (name: string) => {
    if (!bootstrap?.silo) return;
    begin("silo");
    try {
      await invoke("silo_rename", { id: bootstrap.silo.id, name });
      await refreshSilos();
      setBootstrap(await invoke<Bootstrap>("app_bootstrap"));
      toasts.success("Renamed.");
    } catch (e) {
      toasts.error(e);
    } finally {
      end("silo");
    }
  };

  const forgetSilo = async (target?: Silo) => {
    const silo = target ?? bootstrap?.silo;
    if (!silo) return;
    // One question, not two: deleting the files is a box on the same
    // dialog. Asking afterwards meant committing to "Remove" before
    // learning there was a harsher half. The box appears only when the
    // folder is reachable.
    const { ok, option: alsoDelete } = await askConfirmWith(
      "Remove this silo?",
      silo.present
        ? `“${silo.name}” comes out of the list. The folder at ${silo.path} stays where it is, so you can add it back later.`
        : `“${silo.name}” comes out of the list. Its folder isn't reachable right now, so nothing on disk is touched.`,
      {
        confirmLabel: "Remove",
        danger: true,
        option: silo.present
          ? {
              label: "Delete the files as well",
              hint: `Erases everything at ${silo.path} and cannot be undone. Any copy in your backup storage is left behind.`,
            }
          : undefined,
      },
    );
    if (!ok) return;

    begin("silo");
    try {
      await invoke("silo_forget", { id: silo.id, deleteFiles: alsoDelete });
      setMeta(null);
      unlockedRef.current = false;
      await refreshSilos();
      setBootstrap(await invoke<Bootstrap>("app_bootstrap"));
      resetExplorer();
      toasts.success(alsoDelete ? "Silo removed and deleted." : "Silo removed from the list.");
    } catch (e) {
      toasts.error(e);
    } finally {
      end("silo");
    }
  };

  const openSilo = async (id: string) => {
    begin("silo");
    try {
      await invoke("silo_open", { id });
      await refreshSilos();
      resetExplorer();
      const next = await invoke<Bootstrap>("app_bootstrap");
      setBootstrap(next);
      // A silo left unlocked is stepped straight into; only a locked one
      // sends the user to the key prompt, or switching back and forth would
      // ask for a key each way. Through the same path the key prompt ends
      // in, not just `setMeta`: opening a silo means landing in its root
      // folder, and metadata alone leaves the explorer with no current
      // folder, which every action reads before doing anything.
      if (next.locked) {
        setMeta(null);
      } else {
        await finishOpeningSilo(await invoke<VaultMeta>("vault_meta"));
      }
    } catch (e) {
      toasts.error(e);
    } finally {
      end("silo");
    }
  };

  // Back to the picker, with the silo left as it was. Stepping out to
  // reach another silo is not a decision to lock this one; each silo's own
  // timeout handles walking away.
  const closeSilo = async () => {
    begin("silo");
    try {
      await invoke("silo_blur");
      setMeta(null);
      unlockedRef.current = false;
      await refreshSilos();
      setBootstrap(await invoke<Bootstrap>("app_bootstrap"));
      resetExplorer();
    } catch (e) {
      toasts.error(e);
    } finally {
      end("silo");
    }
  };

  const finishOpeningSilo = async (m: VaultMeta) => {
    setMeta(m);
    const root = await invoke<FolderEntry>("vault_root_folder");
    setCurrentFolder(root);
    setEntries(await invoke<VaultEntry[]>("vault_list_folder", { folderId: root.id }));
    setNavHistory([root.id]);
    setNavIndex(0);
    clearSelection();
  };

  const joinSilo = async (meta: unknown) => {
    await refreshSilos();
    setBootstrap(await invoke<Bootstrap>("app_bootstrap"));
    setJoining(false);
    await finishOpeningSilo(meta as VaultMeta);
    toasts.success("This device joined the silo.");
  };

  const enrollPrimaryKey = async (authenticator: Authenticator) => {
    setFidoProgress(null);
    begin("keys");
    try {
      await invoke("fido_enroll_primary", { authenticator });
      const b = await invoke<Bootstrap>("app_bootstrap");
      setBootstrap(b);
      resetExplorer();
      toasts.success(
        authenticator === "this-device"
          ? "Windows Hello enrolled. It opens this silo from now on."
          : "Security key enrolled. It opens this silo from now on.",
      );
    } catch (e) {
      toasts.error(e);
    } finally {
      setFidoProgress(null);
      end("keys");
    }
  };

  const unlockSilo = async (recoveryCodeUsed?: string) => {
    setFidoProgress(null);
    begin("silo");
    try {
      await loadRootExplorer(recoveryCodeUsed);
      await refreshBootstrap();
      await refreshTrash();
      try {
        const keys = await invoke<SecurityKeyInfo[]>("fido_list_keys");
        setSecurityKeys(keys);
      } catch {
        setSecurityKeys([]);
      }

      unlockedRef.current = true;

      // Read, not acted on. A silo without a recovery code is reported by
      // Health, which links to the one screen where setting one up sits
      // beside printing it, so the choice reads as a decision rather than
      // something that happened to the user on the way in.
      try {
        setRecovery(await invoke<RecoveryStatus>("recovery_status"));
      } catch {
        // A silo that cannot report its recovery state is not a reason to
        // block the unlock the user actually asked for.
      }

      await drainShellQueues();
    } catch (e) {
      toasts.error(e);
    } finally {
      setFidoProgress(null);
      end("silo");
    }
  };

  /// Paths queued by the Explorer verbs while the app was locked. Deferred
  /// until the silo is actually usable, and until the recovery step (which
  /// can sit in front of it) is out of the way.
  const drainShellQueues = async () => {
    await checkPendingShellUploads();
    await checkPendingShellDownloads();
  };

  const lockSilo = useCallback(async () => {
    // This silo, not every open one. The button lives in one silo's
    // sidebar, so it reads as a statement about that silo — locking the
    // others too is a decision the user did not make here. Quitting and
    // the per-silo timeouts are what close the rest.
    await invoke("vault_lock", { id: focusedSiloRef.current });
    resetExplorer();
    await refreshBootstrap();
  }, [refreshBootstrap, resetExplorer]);

  /// Backing out of enrolment. The silo was created moments ago and holds
  /// nothing, so discarding it outright is both what the user means and the
  /// only outcome that doesn't leave an unopenable entry in the list — a
  /// silo with no enrolled key cannot be opened again.
  const discardUnenrolledSilo = async () => {
    const silo = bootstrap?.silo;
    if (!silo) return;
    const confirmed = await askConfirm(
      "Discard this silo?",
      `“${silo.name}” has no security key yet and nothing in it, so this deletes the folder at ${silo.path}.`,
      { confirmLabel: "Discard silo", danger: true },
    );
    if (!confirmed) return;
    begin("silo");
    try {
      await invoke("silo_forget", { id: silo.id, deleteFiles: true });
      setMeta(null);
      unlockedRef.current = false;
      await refreshSilos();
      await refreshBootstrap();
      resetExplorer();
    } catch (e) {
      toasts.error(e);
    } finally {
      end("silo");
    }
  };

  /**
   * Auto-lock, once per open silo. Each carries its own timeout, which
   * only works if the clock measures time since *that silo* was used
   * rather than since the window saw a mouse: otherwise a silo sitting
   * open behind the one being worked in never reaches its limit. Rust
   * keeps the elapsed time, and this side contributes the one thing it
   * cannot see, that someone is reading the screen.
   */
  useEffect(() => {
    if (!meta) return;

    // Throttled: the point is to say "still here", and saying it on every
    // mouse move would be thousands of calls to communicate one bit.
    let lastTouch = 0;
    const touch = () => {
      const now = Date.now();
      if (now - lastTouch < 10_000) return;
      lastTouch = now;
      void invoke("silo_touch").catch(() => {});
    };
    window.addEventListener("mousemove", touch);
    window.addEventListener("keydown", touch);
    window.addEventListener("click", touch);

    const sweep = async () => {
      let idle: SiloIdleStatus[];
      try {
        idle = await invoke<SiloIdleStatus[]>("silo_idle_status");
      } catch {
        return;
      }
      for (const silo of idle) {
        if (!shouldLock(silo, autoLockMinutes)) continue;
        try {
          await invoke("vault_lock", { id: silo.id });
        } catch {
          continue;
        }
        // Only the silo on screen changes what is on screen; the others
        // lock quietly, which is what "in the background" means.
        if (silo.id === focusedSiloRef.current) {
          resetExplorer();
          await refreshBootstrap();
        }
      }
    };
    const interval = window.setInterval(() => void sweep(), 15_000);

    return () => {
      window.clearInterval(interval);
      window.removeEventListener("mousemove", touch);
      window.removeEventListener("keydown", touch);
      window.removeEventListener("click", touch);
    };
  }, [meta, autoLockMinutes, refreshBootstrap, resetExplorer]);

  const navigateTo = useCallback(
    async (folderId: string, mode: NavMode = "push") => {
      const folder = await invoke<FolderEntry>("vault_get_folder", { folderId });
      const list = await invoke<VaultEntry[]>("vault_list_folder", { folderId });
      setCurrentFolder(folder);
      setEntries(list);
      clearSelection();
      if (mode === "push") {
        const base = navHistory.slice(0, navIndex + 1);
        const next = base[base.length - 1] === folderId ? base : [...base, folderId];
        setNavHistory(next);
        setNavIndex(next.length - 1);
      } else if (mode === "replace") {
        setNavHistory([folderId]);
        setNavIndex(0);
      }
    },
    [navHistory, navIndex, clearSelection],
  );

  const refreshCurrentFolderRef = useRef<(() => Promise<void>) | null>(null);

  const refreshCurrentFolder = useCallback(async () => {
    if (!currentFolder) return;
    const list = await invoke<VaultEntry[]>("vault_list_folder", {
      folderId: currentFolder.id,
    });
    setEntries(list);
    // The badges belong to these rows, so they are read with them rather
    // than on a timer that would leave a new file unlabelled for seconds.
    try {
      setBlobStatus(await invoke<BlobStatus>("vault_blob_status"));
    } catch {
      /* Locked or mid-teardown; the existing labels stay until the next read. */
    }
  }, [currentFolder]);

  // Kept in a ref so the "vault-changed" listener, which is registered once,
  // always calls the closure holding the folder currently on screen.
  useEffect(() => {
    refreshCurrentFolderRef.current = refreshCurrentFolder;
  }, [refreshCurrentFolder]);

  const goBack = useCallback(async () => {
    if (navIndex <= 0) return;
    const nextIndex = navIndex - 1;
    const folderId = navHistory[nextIndex];
    if (!folderId) return;
    setNavIndex(nextIndex);
    begin("navigate");
    try {
      await navigateTo(folderId, "index");
    } catch (e) {
      toasts.error(e);
    } finally {
      end("navigate");
    }
  }, [begin, end, navHistory, navIndex, navigateTo, toasts]);

  const goForward = useCallback(async () => {
    if (navIndex >= navHistory.length - 1) return;
    const nextIndex = navIndex + 1;
    const folderId = navHistory[nextIndex];
    if (!folderId) return;
    setNavIndex(nextIndex);
    begin("navigate");
    try {
      await navigateTo(folderId, "index");
    } catch (e) {
      toasts.error(e);
    } finally {
      end("navigate");
    }
  }, [begin, end, navHistory, navIndex, navigateTo, toasts]);

  const goUp = useCallback(async () => {
    if (!currentFolder?.parent_id) return;
    begin("navigate");
    try {
      await navigateTo(currentFolder.parent_id, "push");
    } catch (e) {
      toasts.error(e);
    } finally {
      end("navigate");
    }
  }, [begin, end, currentFolder, navigateTo, toasts]);

  const jumpToPath = useCallback(
    async (path: string) => {
      begin("navigate");
      try {
        const folder = await invoke<FolderEntry>("vault_folder_by_path", { path });
        await navigateTo(folder.id, "push");
      } catch (e) {
        toasts.error(e);
      } finally {
        end("navigate");
      }
    },
    [begin, end, navigateTo, toasts],
  );

  const openFolder = useCallback(
    async (folder: Extract<VaultEntry, { kind: "folder" }>) => {
      begin("navigate");
      try {
        await navigateTo(folder.id, "push");
      } catch (e) {
        toasts.error(e);
      } finally {
        end("navigate");
      }
    },
    [begin, end, navigateTo, toasts],
  );

  const handleSelectClick = useCallback(
    (entry: VaultEntry, e: MouseEvent) => {
      const idx = entries.findIndex((x) => x.id === entry.id);
      if (e.shiftKey && anchorIndex !== null && idx >= 0) {
        const [a, b] = anchorIndex < idx ? [anchorIndex, idx] : [idx, anchorIndex];
        const next = new Set<string>();
        for (let i = a; i <= b; i++) next.add(entries[i]!.id);
        setSelectedIds(next);
        return;
      }
      if (e.ctrlKey || e.metaKey) {
        setSelectedIds((prev) => {
          const next = new Set(prev);
          if (next.has(entry.id)) next.delete(entry.id);
          else next.add(entry.id);
          return next;
        });
        setAnchorIndex(idx >= 0 ? idx : null);
        return;
      }
      setSelectedIds(new Set([entry.id]));
      setAnchorIndex(idx >= 0 ? idx : null);
    },
    [entries, anchorIndex],
  );

  const handleCreateFolder = async () => {
    if (!currentFolder || !newFolderName.trim()) return;
    begin("entry");
    try {
      await invoke("vault_create_folder", {
        parentId: currentFolder.id,
        name: newFolderName.trim(),
      });
      setNewFolderName("");
      await refreshCurrentFolder();
      toasts.success("Folder created.");
    } catch (e) {
      toasts.error(e);
    } finally {
      end("entry");
    }
  };

  const handleAddFiles = async () => {
    if (!currentFolder) return;
    const picked = await open({ multiple: true });
    if (!picked) return;
    const paths = Array.isArray(picked) ? picked : [picked];
    if (paths.length === 0) return;
    if (!(await roomFor(paths))) return;

    const folderId = currentFolder.id;
    const concurrency = uploadConcurrency();
    const total = paths.length;
    const counters = { imported: 0, failed: 0 };
    const failed: string[] = [];

    const refreshProgress = () => {
      setUploadProgress(`Encrypting ${counters.imported + counters.failed}/${total}…`);
    };

    uploadCancelRef.current = false;
    setUploadCancelable(true);
    setUploadCancelling(false);
    begin("transfer");
    refreshProgress();
    try {
      await mapPool(
        paths,
        concurrency,
        async (sourcePath) => {
        const name = sourcePath.replace(/^.*[/\\]/, "");
        try {
          await invoke<FileEntry>("vault_import_file", {
            folderId,
            sourcePath,
          });
          counters.imported += 1;
          refreshProgress();
          // Show the file in the list immediately.
          await refreshCurrentFolder();
          refreshProgress();
        } catch (e) {
          counters.failed += 1;
          failed.push(`${name}: ${String(e)}`);
          refreshProgress();
        }
        },
        () => uploadCancelRef.current,
      );

      await refreshCurrentFolder();
      const wasCancelled = uploadCancelRef.current && counters.imported + counters.failed < total;
      if (wasCancelled) {
        // toasts.info (not .error) — .error runs the message through
        // formatAppError, which rewrites anything containing "cancelled"
        // into a FIDO-prompt message unrelated to this import.
        toasts.info(
          `Cancelled. ${counters.imported} added, ${total - counters.imported - counters.failed} skipped.`,
        );
      } else if (failed.length === 0) {
        toasts.success(
          counters.imported === 1 ? "Added 1 file." : `Added ${counters.imported} files.`,
        );
      } else {
        toasts.error(
          `Added ${counters.imported} of ${total}. Failed: ${failed.slice(0, 3).join("; ")}${failed.length > 3 ? "…" : ""}`,
        );
      }
    } finally {
      setUploadProgress(null);
      end("transfer");
      setUploadCancelable(false);
      setUploadCancelling(false);
    }
  };

  // Covers both cancellation mechanisms in play: the JS-side ref (checked by
  // mapPool for handleAddFiles's per-file loop) and the Rust-side flag
  // (checked between items by the folder-import/paste commands, which run
  // as one blocking invoke so a JS ref alone can't reach them). Only one of
  // the two is ever actually driving the current operation; the other is a
  // harmless no-op.
  const cancelUpload = () => {
    uploadCancelRef.current = true;
    setUploadCancelling(true);
    void invoke("cancel_import").catch((e) => {
      // Not swallowed silently: if this rejects (e.g. a stale build that
      // predates this command), the Rust-side flows would otherwise just
      // run to completion with no visible sign the click did nothing.
      console.error("cancel_import failed — Rust-side cancellation may not have registered:", e);
    });
  };

  const handleAddFolder = async () => {
    if (!currentFolder) return;
    const picked = await open({ directory: true, multiple: false });
    if (!picked) return;
    const sourcePath = typeof picked === "string" ? picked : picked[0];
    if (!sourcePath) return;
    // Measured by walking the tree, which is the only honest estimate for a
    // folder and still far cheaper than encrypting it to find out.
    if (!(await roomFor([sourcePath]))) return;

    await invoke("reset_import_cancel").catch(() => {});
    setUploadCancelable(true);
    setUploadCancelling(false);
    begin("transfer");
    setUploadProgress("Scanning folder…");

    const unlisten = await listen<{
      phase: string;
      current: number;
      total: number;
      name: string;
    }>("import-progress", (event) => {
      const { phase, current, total, name } = event.payload;
      if (phase === "scanning") {
        setUploadProgress(
          total > 0 ? `Scanning… ${total} files found` : "Scanning folder…",
        );
      } else if (phase === "folders") {
        setUploadProgress(`Creating folders… ${name}`);
      } else if (phase === "encrypting") {
        setUploadProgress(
          total > 0
            ? `Encrypting ${current}/${total}: ${name}`
            : `Encrypting: ${name}`,
        );
      } else if (phase === "done") {
        setUploadProgress(
          total > 0 ? `Imported ${current}/${total} files` : "Import complete",
        );
      }
    });

    const unlistenSync = await listen<[number, number]>("blob-sync-progress", (event) => {
      const [current, total] = event.payload;
      setUploadProgress(
        total > 0 ? `Backing up… ${current}/${total} files` : "Backing up…",
      );
    });

    try {
      await invoke("vault_import_folder", {
        folderId: currentFolder.id,
        sourcePath,
      });
      await refreshCurrentFolder();

      toasts.success("Folder imported.");
    } catch (e) {
      if (String(e) === "cancelled") {
        await refreshCurrentFolder();
        // toasts.info, not .error — see the comment on the equivalent
        // branch in handleAddFiles for why.
        toasts.info("Cancelled. Part of the folder was added before you stopped it.");
      } else {
        toasts.error(e);
      }
    } finally {
      unlisten();
      unlistenSync();
      setUploadProgress(null);
      end("transfer");
      setUploadCancelable(false);
      setUploadCancelling(false);
    }
  };

  // Ctrl+V in the file explorer: paste whatever files/folders are on the OS
  // clipboard (e.g. copied with Ctrl+C in Windows Explorer). A no-op if the
  // clipboard doesn't hold any files — this must never fight with normal
  // Ctrl+V text-paste elsewhere in the app.
  const handlePasteFiles = async () => {
    const paths = await invoke<string[]>("clipboard_file_paths").catch(() => []);
    if (paths.length === 0) return;
    await importPaths(paths, "Pasted");
  };

  /// Asks the disk whether what is about to be imported fits, measured
  /// before the first byte is encrypted: an import that fills the disk
  /// leaves a half-written blob and no room to fix it. The number is a
  /// snapshot, so this warns and asks rather than refusing outright.
  /// `ask` is false where a dialog is already on screen, because stacked
  /// modals leave the one underneath still taking clicks. A refusal is
  /// still a refusal either way.
  const roomFor = async (paths: string[], ask = true): Promise<boolean> => {
    let report: SpaceReport;
    try {
      report = await invoke<SpaceReport>("vault_disk_space", { paths });
    } catch {
      // A disk that cannot be asked is not a disk that is full. Blocking an
      // import on a failed query would be the app inventing a problem.
      return true;
    }
    setDiskSpace(report);

    if (report.verdict === "insufficient") {
      const short = report.wanted_bytes - (report.available_bytes ?? 0);
      toasts.error(
        `Not enough room: this needs about ${formatBytes(report.wanted_bytes)} and the disk has ` +
          `${formatBytes(report.available_bytes ?? 0)} left, about ${formatBytes(short)} short.`,
      );
      return false;
    }

    if (report.verdict === "tight") {
      if (!ask) {
        toasts.info("This will leave very little room on the disk where the silo lives.");
        return true;
      }
      const ok = await askConfirm(
        "This will nearly fill the disk",
        `Adding this leaves under ${formatBytes(report.headroom_bytes)} free where the silo lives. ` +
          "Windows needs room of its own to keep working, and the silo needs room to record what " +
          "changed.",
        { confirmLabel: "Add anyway" },
      );
      if (!ok) return false;
    }

    // Bought space is the other limit, and running out shows up as a sync
    // that stops rather than an import that fails. A warning rather than a
    // refusal: the figure is counted on a schedule and can be hours old.
    if (hostedUsage && hostedUsage.quota_bytes > 0) {
      const left = Math.max(0, hostedUsage.quota_bytes - hostedUsage.account_bytes);
      if (report.wanted_bytes > left) {
        // Both sides in the units the space was sold in, so the two numbers
        // in one sentence can actually be compared.
        const message =
          `This is about ${formatStorage(report.wanted_bytes)} and your SilentSilo storage has ` +
          `${formatStorage(left)} left, so some of it will not finish backing up. The figure is ` +
          "counted by the service and can be a few hours behind. Change the plan from your " +
          "account page, or remove what you no longer need.";
        if (!ask) {
          toasts.info(message);
          return true;
        }
        return await askConfirm("More than your storage has room for", message, {
          confirmLabel: "Add anyway",
        });
      }
    }

    return true;
  };

  /// Shared by clipboard paste and window drop: the same set of OS paths,
  /// landing in the folder on screen, with the same progress and the same
  /// per-item failure reporting.
  const importPaths = async (paths: string[], verb: "Pasted" | "Added") => {
    if (!currentFolder || paths.length === 0) return;
    if (!(await roomFor(paths))) return;

    const folderId = currentFolder.id;
    await invoke("reset_import_cancel").catch(() => {});
    setUploadCancelable(true);
    setUploadCancelling(false);
    begin("transfer");
    setUploadProgress(verb === "Pasted" ? "Pasting…" : "Adding…");

    const unlisten = await listen<{
      phase: string;
      current: number;
      total: number;
      name: string;
    }>("import-progress", (event) => {
      const { phase, current, total, name } = event.payload;
      if (phase === "scanning") {
        setUploadProgress(total > 0 ? `Scanning… ${total} files found` : "Scanning…");
      } else if (phase === "folders") {
        setUploadProgress(`Creating folders… ${name}`);
      } else if (phase === "encrypting") {
        setUploadProgress(
          total > 0 ? `Encrypting ${current}/${total}: ${name}` : `Encrypting: ${name}`,
        );
      } else if (phase === "done") {
        setUploadProgress(total > 0 ? `Imported ${current}/${total} files` : "Import complete");
      }
    });
    const unlistenSync = await listen<[number, number]>("blob-sync-progress", (event) => {
      const [current, total] = event.payload;
      setUploadProgress(
        total > 0 ? `Backing up… ${current}/${total} files` : "Backing up…",
      );
    });

    try {
      const result = await invoke<{
        imported_files: number;
        imported_folders: number;
        failed: string[];
      }>("vault_paste_paths", { folderId, paths });
      await refreshCurrentFolder();

      const totalImported = result.imported_files + result.imported_folders;
      if (result.failed.length > 0) {
        toasts.error(
          `${verb} ${totalImported} ${totalImported === 1 ? "item" : "items"}. Failed: ${result.failed.slice(0, 3).join("; ")}${result.failed.length > 3 ? "…" : ""}`,
        );
      } else if (totalImported > 0) {
        toasts.success(
          totalImported === 1 ? `${verb} 1 item.` : `${verb} ${totalImported} items.`,
        );
      }
    } catch (e) {
      toasts.error(e);
    } finally {
      unlisten();
      unlistenSync();
      setUploadProgress(null);
      end("transfer");
      setUploadCancelable(false);
      setUploadCancelling(false);
    }
  };

  const confirmShellUpload = async (folderId: string) => {
    if (!pendingShellUploadPaths) return;
    if (!(await roomFor(pendingShellUploadPaths, false))) return;
    setShellUploadBusy(true);
    try {
      // The same command paste and drop use, because the shell hands over
      // whatever was right-clicked and that is as often a folder as a file.
      // This used to import files only, so choosing a folder from the
      // context menu got as far as asking where to put it and then said
      // "not a file", after a security key touch.
      await invoke("reset_import_cancel").catch(() => {});
      const result = await invoke<{
        imported_files: number;
        imported_folders: number;
        failed: string[];
      }>("vault_paste_paths", {
        folderId,
        paths: pendingShellUploadPaths,
      });
      if (currentFolder?.id === folderId) {
        await refreshCurrentFolder();
      }
      setPendingShellUploadPaths(null);

      const total = result.imported_files + result.imported_folders;
      if (total > 0) {
        toasts.success(total === 1 ? "Added 1 item." : `Added ${total} items.`);
      }
      // Reported rather than folded into the count: an item that did not
      // arrive is the one thing the user needs to know about.
      for (const failure of result.failed) {
        toasts.error(failure);
      }
      if (total === 0 && result.failed.length === 0) {
        toasts.info("Nothing was added.");
      }
    } catch (e) {
      toasts.error(e);
    } finally {
      setShellUploadBusy(false);
    }
  };

  const cancelShellUpload = () => {
    const count = pendingShellUploadPaths?.length ?? 0;
    setPendingShellUploadPaths(null);
    if (count > 0) {
      toasts.info(
        count === 1
          ? "Cancelled. 1 item was not added."
          : `Cancelled. ${count} items were not added.`,
      );
    }
  };

  const confirmShellDownload = async (selection: VaultEntry[]) => {
    if (!pendingShellDownloadTarget || selection.length === 0) return;
    const destDir = pendingShellDownloadTarget;
    setShellDownloadBusy(true);
    try {
      let count = 0;
      const failed: string[] = [];
      for (const entry of selection) {
        try {
          if (entry.kind === "folder") {
            count += await invoke<number>("vault_export_folder", {
              folderId: entry.id,
              destDir,
            });
          } else {
            const destPath = await join(destDir, entry.name);
            await invoke("vault_export_file", { fileId: entry.id, destPath });
            count += 1;
          }
        } catch (e) {
          failed.push(`${entry.name}: ${String(e)}`);
        }
      }
      setPendingShellDownloadTarget(null);
      if (failed.length > 0) {
        toasts.error(
          `Saved ${count} ${count === 1 ? "item" : "items"}. Failed: ${failed.slice(0, 3).join("; ")}${failed.length > 3 ? "…" : ""}`,
        );
      } else {
        toasts.success(count === 1 ? "Saved 1 item." : `Saved ${count} items.`);
      }
    } finally {
      setShellDownloadBusy(false);
    }
  };

  const cancelShellDownload = () => {
    setPendingShellDownloadTarget(null);
  };

  const selectedEntries = useMemo(
    () => entries.filter((e) => selectedIds.has(e.id)),
    [entries, selectedIds],
  );

  /**
   * Moves the selection to the trash, without asking first. Nothing is
   * destroyed: Restore puts it back. A dialog in front of a reversible
   * action buys no safety and trains the user to dismiss dialogs, which is
   * what makes the irreversible one less safe. Empty trash still asks.
   */
  const trashEntries_ = async (targets: VaultEntry[]) => {
    if (targets.length === 0) return;
    begin("entry");
    try {
      for (const entry of targets) {
        if (entry.kind === "folder") {
          await invoke("vault_trash_folder", { folderId: entry.id });
        } else {
          await invoke("vault_trash_file", { fileId: entry.id });
        }
      }
      clearSelection();
      await refreshCurrentFolder();
      await refreshTrash();
      toasts.info(
        targets.length === 1
          ? `Moved “${targets[0]!.name}” to Trash.`
          : `Moved ${targets.length} items to Trash.`,
      );
    } catch (e) {
      toasts.error(e);
    } finally {
      end("entry");
    }
  };

  const handleTrash = async () => {
    if (!currentFolder) return;
    await trashEntries_(selectedEntries);
  };

  // Context-menu variant: acts on a single entry directly rather than the
  // current left-click selection, so right-clicking an unselected item
  // doesn't select it or pop the selection toolbar.
  const handleTrashEntry = async (entry: VaultEntry) => {
    await trashEntries_([entry]);
  };

  // Context-menu variant of startRename — same reasoning as handleTrashEntry.
  const startRenameEntry = (entry: VaultEntry) => {
    setRenamingId(entry.id);
    setRenameValue(entry.name);
  };

  const handleRestore = async (entry: VaultEntry) => {
    begin("entry");
    try {
      if (entry.kind === "folder") {
        await invoke("vault_restore_folder", { folderId: entry.id });
      } else {
        await invoke("vault_restore_file", { fileId: entry.id });
      }
      await refreshTrash();
      toasts.success(`Restored "${entry.name}".`);
    } catch (e) {
      toasts.error(e);
    } finally {
      end("entry");
    }
  };

  /// Restoring several is one report, not one toast per item: a batch is
  /// what the user asked for, so a batch is what they should be told about.
  const handleRestoreMany = async (items: TrashItem[]) => {
    begin("entry");
    let failed = 0;
    try {
      for (const entry of items) {
        try {
          if (entry.kind === "folder") {
            await invoke("vault_restore_folder", { folderId: entry.id });
          } else {
            await invoke("vault_restore_file", { fileId: entry.id });
          }
        } catch {
          failed += 1;
        }
      }
      await refreshTrash();
      const done = items.length - failed;
      if (failed > 0) {
        toasts.error(`Restored ${done} of ${items.length}. The rest can be retried.`);
      } else {
        toasts.success(done === 1 ? "Restored 1 item." : `Restored ${done} items.`);
      }
    } finally {
      end("entry");
    }
  };

  /// Permanent deletion of a selection rather than the whole trash. The
  /// confirmation names the count and says it cannot be undone, which is
  /// the only warning there is: nothing here goes to a second bin.
  const handleDeleteForever = async (items: TrashItem[]) => {
    if (items.length === 0) return;
    const what =
      items.length === 1 ? `“${items[0]!.name}”` : `${items.length} items`;
    const folders = items.filter((e) => e.kind === "folder").length;
    const foldersNote =
      folders > 0
        ? ` Everything inside ${folders === 1 ? "the folder" : "the folders"} goes too.`
        : "";
    const confirmed = await askConfirm(
      "Delete permanently?",
      `This deletes ${what} for good. It cannot be undone.${foldersNote}${SUPERSEDED_NOTE}${archiveNote(archiveTargets)}`,
      { confirmLabel: deleteForGoodLabel(archiveTargets), danger: true },
    );
    if (!confirmed) return;

    begin("entry");
    try {
      await invoke<number>("vault_purge_items", { ids: items.map((e) => e.id) });
      await refreshTrash();
      await refreshBlobStatus();
      const verb = archiveTargets > 0 ? "Deleted" : "Permanently deleted";
      toasts.success(
        items.length === 1 ? `${verb} 1 item.` : `${verb} ${items.length} items.`,
      );
    } catch (e) {
      toasts.error(e);
    } finally {
      end("entry");
    }
  };

  const handleEmptyTrash = async () => {
    if (trashEntries.length === 0) return;
    const confirmed = await askConfirm(
      "Empty the trash?",
      `This deletes ${trashEntries.length} ${trashEntries.length === 1 ? "item" : "items"} for good. It cannot be undone.${SUPERSEDED_NOTE}${archiveNote(archiveTargets)}`,
      { confirmLabel: deleteForGoodLabel(archiveTargets), danger: true },
    );
    if (!confirmed) return;
    begin("entry");
    try {
      const removed = await invoke<number>("vault_empty_trash");
      await refreshTrash();
      const verb = archiveTargets > 0 ? "Deleted" : "Permanently deleted";
      toasts.success(removed === 1 ? `${verb} 1 item.` : `${verb} ${removed} items.`);
    } catch (e) {
      toasts.error(e);
    } finally {
      end("entry");
    }
  };

  const startRename = () => {
    if (selectedEntries.length !== 1) return;
    const entry = selectedEntries[0]!;
    setRenamingId(entry.id);
    setRenameValue(entry.name);
  };

  const cancelRename = () => {
    setRenamingId(null);
    setRenameValue("");
  };

  const commitRename = async () => {
    if (!renamingId) return;
    const entry = entries.find((e) => e.id === renamingId);
    const next = renameValue.trim();
    if (!entry || !next || next === entry.name) {
      cancelRename();
      return;
    }
    begin("entry");
    try {
      if (entry.kind === "folder") {
        await invoke("vault_rename_folder", { folderId: entry.id, newName: next });
      } else {
        await invoke("vault_rename_file", { fileId: entry.id, newName: next });
      }
      cancelRename();
      await refreshCurrentFolder();
    } catch (e) {
      toasts.error(e);
    } finally {
      end("entry");
    }
  };

  // Writing a decrypted copy out to the computer. Never called a download:
  // the file has been on this machine all along, and the only thing that
  // changes here is that a plaintext copy leaves the silo.
  const handleSaveCopy = async (file: Extract<VaultEntry, { kind: "file" }>) => {
    const dest = await save({ defaultPath: file.name });
    if (!dest) return;
    begin("transfer");
    try {
      await invoke("vault_export_file", { fileId: file.id, destPath: dest });
      toasts.success("Saved a copy.");
    } catch (e) {
      toasts.error(e);
    } finally {
      end("transfer");
    }
  };

  const handleSaveCopies = async (files: Extract<VaultEntry, { kind: "file" }>[]) => {
    const destDir = await open({ directory: true, multiple: false });
    if (!destDir) return;
    const dest = typeof destDir === "string" ? destDir : destDir[0];
    if (!dest) return;

    begin("transfer");
    let saved = 0;
    try {
      for (const file of files) {
        const destPath = await join(dest, file.name);
        try {
          await invoke("vault_export_file", { fileId: file.id, destPath });
          saved++;
        } catch (e) {
          toasts.error(e);
        }
      }
      if (saved > 0) {
        toasts.success(saved === 1 ? "Saved 1 file." : `Saved ${saved} files.`);
      }
    } finally {
      end("transfer");
    }
  };

  const handleSaveFolder = async (folder: Extract<VaultEntry, { kind: "folder" }>) => {
    const destDir = await open({ directory: true, multiple: false });
    if (!destDir) return;
    const dest = typeof destDir === "string" ? destDir : destDir[0];
    if (!dest) return;

    begin("transfer");
    setUploadProgress(`Saving “${folder.name}”…`);
    const unlisten = await listen<number>("export-progress", (event) => {
      setUploadProgress(
        `Saving “${folder.name}”… ${event.payload} ${event.payload === 1 ? "file" : "files"}`,
      );
    });
    try {
      const count = await invoke<number>("vault_export_folder", {
        folderId: folder.id,
        destDir: dest,
      });
      toasts.success(count === 1 ? "Saved 1 file." : `Saved ${count} files.`);
    } catch (e) {
      toasts.error(e);
    } finally {
      unlisten();
      setUploadProgress(null);
      end("transfer");
    }
  };

  // Debounced, because every keystroke would otherwise be a query. 180ms is
  // below the threshold where typing feels laggy and well above the interval
  // between characters, so a normal word costs one query rather than five.
  const searchTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const runSearch = (query: string) => {
    if (searchTimer.current) clearTimeout(searchTimer.current);
    if (!query.trim()) {
      setGlobalResults(null);
      setSearching(false);
      return;
    }
    setSearching(true);
    searchTimer.current = setTimeout(() => {
      void invoke<SearchHit[]>("vault_search", { query })
        .then(setGlobalResults)
        .catch(() => setGlobalResults([]))
        .finally(() => setSearching(false));
    }, 180);
  };

  // Jumping to a hit ends the search: the user found what they wanted, and
  // leaving results on screen would hide the folder they just navigated to.
  const jumpToHit = async (hit: SearchHit) => {
    setGlobalResults(null);
    setSearching(false);
    const folderId = hit.kind === "folder" ? hit.id : hit.folder_id;
    await navigateTo(folderId, "push");
    if (hit.kind === "file") setSelectedIds(new Set([hit.id]));
  };

  /// Starred files and folders, read from the index rather than derived from
  /// what happens to be on screen: a favourite in a folder nobody has opened
  /// this session still belongs in the list.
  const refreshFavorites = useCallback(async () => {
    try {
      setFavoriteHits(await invoke<SearchHit[]>("vault_list_favorites"));
    } catch (e) {
      toasts.error(e);
    }
  }, [toasts]);

  /// Read from the operation log, so it is only as complete as the last
  /// sync: a device whose records have not arrived here yet is not in it.
  const refreshDevices = useCallback(async () => {
    try {
      setDevices(await invoke<DeviceInfo[]>("vault_list_devices"));
    } catch {
      // Locked or mid-teardown. The page shows what it last read.
    }
  }, []);

  const renameDevice = useCallback(
    async (deviceId: string, label: string) => {
      begin("entry");
      try {
        await invoke("vault_set_device_label", { deviceId, label });
        await refreshDevices();
      } catch (e) {
        toasts.error(e);
      } finally {
        end("entry");
      }
    },
    [begin, end, refreshDevices, toasts],
  );

  const toggleFavorite = useCallback(
    async (entry: VaultEntry) => {
      begin("entry");
      try {
        await invoke("vault_set_favorite", {
          id: entry.id,
          kind: entry.kind,
          favorite: !entry.favorite,
        });
        await Promise.all([refreshCurrentFolder(), refreshFavorites()]);
      } catch (e) {
        toasts.error(e);
      } finally {
        end("entry");
      }
    },
    [begin, end, refreshCurrentFolder, refreshFavorites, toasts],
  );

  /**
   * Opening a file from the silo hands it to the system handler, exactly as
   * Explorer would. The difference is where it came from: a silo syncs, so a
   * file can arrive from another device, and someone double-clicking an item
   * in a list of their own documents is not thinking about what its extension
   * does. Asked rather than refused, because these are the user's own files.
   */
  const confirmRun = useCallback(
    (name: string) =>
      askConfirm(
        "Run this file?",
        `“${name}” is a program or a shortcut, so opening it runs it rather than showing it. Only continue if you know where it came from.`,
        { confirmLabel: "Run it", danger: true },
      ),
    [askConfirm],
  );

  const openFile = async (file: Extract<VaultEntry, { kind: "file" }>) => {
    if (runsOnOpen(file.name) && !(await confirmRun(file.name))) return;
    begin("transfer");
    try {
      await invoke("vault_open_file", { fileId: file.id });
    } catch (e) {
      toasts.error(e);
    } finally {
      end("transfer");
    }
  };

  /// Read on the same beat as the sync status, because the two describe one
  /// thing: the sync counters say what the log still owes, this says what
  /// the blobs still owe and what they cost on disk.
  /// Sets, so the explorer can ask about one file without scanning an array
  /// per row. Rebuilt only when the status itself changes.
  const localBlobIds = useMemo(
    () => new Set(blobStatus?.local ?? []),
    [blobStatus],
  );
  const unsyncedBlobIds = useMemo(
    () => new Set(blobStatus?.unsynced ?? []),
    [blobStatus],
  );

  const refreshBlobStatus = useCallback(async () => {
    try {
      setBlobStatus(await invoke<BlobStatus>("vault_blob_status"));
    } catch {
      setBlobStatus(null);
    }
  }, []);

  /// Downloads every piece of content the silo has and this disk does not.
  /// A sync pass moves the index, never the content, so a device that
  /// joined or recovered has the whole tree and none of the bytes: the
  /// right default on a laptop and the wrong one for someone who has just
  /// lost a machine, so it is offered rather than assumed. One blob at a
  /// time in a small pool, so stopping part way keeps what landed.
  const fetchAllContent = useCallback(async () => {
    const ids = blobStatus?.missing ?? [];
    if (ids.length === 0 || contentFetch) return;

    contentFetchCancelRef.current = false;
    setContentFetch({ done: 0, total: ids.length });
    let failed = 0;
    try {
      await mapPool(
        ids,
        uploadConcurrency(),
        async (blobId) => {
          try {
            await invoke("sync_fetch_blob", { blobId });
          } catch {
            // One unreachable object should not abandon the rest: the
            // missing ones are still missing next time, and this is a pass
            // that can simply be run again.
            failed += 1;
          }
          setContentFetch((prev) => (prev ? { ...prev, done: prev.done + 1 } : prev));
        },
        () => contentFetchCancelRef.current,
      );

      if (contentFetchCancelRef.current) {
        toasts.info("Stopped. What had already downloaded is on this computer.");
      } else if (failed > 0) {
        toasts.error(`Downloaded ${ids.length - failed} of ${ids.length}. The rest can be retried.`);
      } else {
        toasts.success(
          ids.length === 1 ? "1 file downloaded." : `${ids.length} files downloaded.`,
        );
      }
    } finally {
      setContentFetch(null);
      await refreshBlobStatus();
    }
  }, [blobStatus, contentFetch, refreshBlobStatus, toasts]);

  const cancelFetchAllContent = useCallback(() => {
    contentFetchCancelRef.current = true;
  }, []);

  /// Whether the explorer's offer to download everything has been waved
  /// away for this silo. Per silo and remembered, because the answer is
  /// about this machine's relationship with this silo, not this session.
  /// The Backup page carries the offer permanently either way.
  const contentOfferKey = meta ? `silentsilo.content.offered.${meta.vault_id}` : null;
  const [contentOfferDismissed, setContentOfferDismissed] = useState(false);
  useEffect(() => {
    setContentOfferDismissed(
      contentOfferKey ? localStorage.getItem(contentOfferKey) === "true" : false,
    );
  }, [contentOfferKey]);

  const dismissContentOffer = useCallback(() => {
    if (contentOfferKey) localStorage.setItem(contentOfferKey, "true");
    setContentOfferDismissed(true);
  }, [contentOfferKey]);


  const refreshSync = useCallback(async () => {
    try {
      const status = await invoke<{
        configured: boolean;
        pending_ops: number;
        archive_targets: number;
      }>("sync_status");
      setSync((prev) => ({
        ...prev,
        configured: status.configured,
        pending: status.pending_ops,
      }));
      setArchiveTargets(status.archive_targets);
    } catch {
      setSync((prev) => ({ ...prev, configured: false }));
    }
    await refreshBlobStatus();
  }, [refreshBlobStatus]);

  const syncNow = async () => {
    setSync((prev) => ({ ...prev, state: "syncing", lastError: null }));
    try {
      const report = await invoke<{ needs_rebuild: boolean }>("sync_now");
      // Nothing moved in either direction, so this is not a pass that
      // succeeded quietly: say so rather than showing a green tick.
      if (report.needs_rebuild) {
        setNeedsRebuild(true);
        setSync((prev) => ({ ...prev, state: "idle" }));
        return;
      }
      setSync((prev) => ({ ...prev, state: "ok", lastSyncAt: Date.now(), lastError: null }));
      await refreshSync();
      await refreshCurrentFolder();
    } catch (e) {
      setSync((prev) => ({ ...prev, state: "error", lastError: String(e) }));
      toasts.error(e);
    }
  };

  const refreshRecovery = useCallback(async () => {
    try {
      setRecovery(await invoke<RecoveryStatus>("recovery_status"));
    } catch {
      setRecovery({ enabled: false, created_at: null });
    }
  }, []);

  const generateRecoveryCode = async () => {
    if (recovery.enabled) {
      const ok = await askConfirm(
        "Replace the recovery code?",
        "The code you wrote down stops working, on every device.",
        { confirmLabel: "Replace the code" },
      );
      if (!ok) return;
    }
    begin("keys");
    try {
      setRecoveryCode(await invoke<string>("recovery_generate"));
      await refreshRecovery();
    } catch (e) {
      toasts.error(e);
    } finally {
      end("keys");
    }
  };

  const disableRecovery = async () => {
    const ok = await askConfirm(
      "Turn off the recovery code?",
      "The code you wrote down stops working on every device, leaving your security keys as the only way in.",
      { confirmLabel: "Turn off recovery", danger: true },
    );
    if (!ok) return;
    begin("keys");
    try {
      const withheld = await invoke<string[]>("recovery_disable");
      await refreshRecovery();
      if (withheld.length > 0) {
        // The envelope is gone from every copy the app deletes from, and
        // still readable on the ones it never deletes from. Someone turning
        // recovery off is doing it because a piece of paper is somewhere it
        // should not be, so this is the moment to be exact.
        toasts.info(
          `Recovery code turned off. It stays readable on ${withheld.join(", ")}, which never deletes anything, so the written code still opens the silo for anyone who can read that storage. Changing that means rotating the silo's key.`,
        );
      } else {
        toasts.success("Recovery code turned off.");
      }
    } catch (e) {
      toasts.error(e);
    } finally {
      end("keys");
    }
  };

  /// A key change left half done, which makes syncing fail until it is
  /// finished. Asked once the silo is open, because the answer is a file in
  /// the silo folder.
  const [rotationPending, setRotationPending] = useState(false);

  /// Carries an interrupted key change to the end. Forward only: once
  /// objects in storage have been re-sealed, going back is not possible.
  const resumeRotation = async (credential: string) => {
    begin("keys");
    try {
      const resealed = await invoke<number>("vault_rotate_resume", { credential });
      setRotationPending(false);
      toasts.success(
        `Key change finished, ${resealed} objects re-sealed. Unlock again with the key you just used.`,
      );
    } catch (e) {
      toasts.error(e);
    } finally {
      end("keys");
    }
  };

  /// Changes the key the whole silo is encrypted under.
  ///
  /// Every key kept has to be touched, so this runs for as long as that takes
  /// and the live instruction names which one. Afterwards the silo locks: the
  /// open session holds the key that just stopped being current.
  const rotateVaultKey = async (keep: string[]) => {
    const ok = await askConfirm(
      "Change this silo's encryption key?",
      "Every key you did not tick stops opening this silo, and your recovery code is replaced. You will be asked to touch each key you are keeping, then the silo locks and you unlock it again.",
      { confirmLabel: "Change the key", danger: true },
    );
    if (!ok) return;

    begin("keys");
    try {
      const outcome = await invoke<{
        resealed: number;
        retired: string[];
        recovery_code: string;
        unchanged_targets: string[];
      }>("vault_rotate_key", { keep });

      // Shown the way a freshly generated one is, because it is one: the old
      // code unwrapped the old key and stopped working just now. The screen
      // moves to where it is displayed, or a code shown once would be shown
      // on a page the user is not looking at.
      setRecoveryCode(outcome.recovery_code);
      setSettingsSection("recovery");
      setRotationPending(false);

      const parts = [`Encryption key changed, ${outcome.resealed} objects re-sealed.`];
      if (outcome.retired.length > 0) {
        parts.push(`${outcome.retired.join(", ")} no longer opens this silo.`);
      }
      if (outcome.unchanged_targets.length > 0) {
        // The one case where rotation does not finish the job, and it has to
        // be said: an append-only copy cannot be overwritten, so what is
        // already there stays readable with the old key.
        parts.push(
          `${outcome.unchanged_targets.join(", ")} never deletes anything, so what is already there still opens with the old key.`,
        );
      }
      toasts.success(parts.join(" "));
    } catch (e) {
      toasts.error(e);
    } finally {
      end("keys");
    }
  };

  const addSecurityKey = async (authenticator: Authenticator) => {
    const label = newKeyLabel.trim();
    if (
      label &&
      securityKeys.some((k) => (k.label || "").toLowerCase() === label.toLowerCase())
    ) {
      const ok = await askConfirm(
        "Use that label twice?",
        `A key labelled “${label}” is already enrolled. Two entries with the same name are hard to tell apart later.`,
        { confirmLabel: "Add anyway" },
      );
      if (!ok) return;
    }

    setKeyAddSuccess(null);
    setFidoProgress("Insert the new security key, then follow the Windows prompts…");
    begin("keys");
    try {
      const added = await invoke<SecurityKeyInfo>("fido_add_key", {
        label: label || null,
        authenticator,
      });
      setNewKeyLabel("");
      const keys = await invoke<SecurityKeyInfo[]>("fido_list_keys");
      setSecurityKeys(keys);
      const b = await invoke<Bootstrap>("app_bootstrap");
      setBootstrap(b);
      setKeyAddSuccess(
        `Added “${securityKeyDisplayName(added)}”. That key can unlock this silo.`,
      );
      toasts.success("Security key added.");
    } catch (e) {
      toasts.error(e);
    } finally {
      setFidoProgress(null);
      end("keys");
    }
  };

  /// Renaming is local to this device until the next sync pass, which
  /// republishes the key envelopes with the label on them.
  const renameSecurityKey = async (credentialId: string, label: string) => {
    begin("keys");
    try {
      await invoke("fido_rename_key", { credentialId, label });
      setSecurityKeys(await invoke<SecurityKeyInfo[]>("fido_list_keys"));
      toasts.success("Security key renamed.");
    } catch (e) {
      toasts.error(e);
    } finally {
      end("keys");
    }
  };

  const removeSecurityKey = async (credentialId: string) => {
    const ok = await askConfirm(
      "Remove this security key?",
      "It stops opening this silo. Make sure another key or a recovery code still can.",
      { confirmLabel: "Remove", danger: true },
    );
    if (!ok) return;
    begin("keys");
    try {
      const outcome = await invoke<{ published: boolean; withheld: string[] }>("fido_remove_key", {
        credentialId,
      });
      const keys = await invoke<SecurityKeyInfo[]>("fido_list_keys");
      setSecurityKeys(keys);
      const b = await invoke<Bootstrap>("app_bootstrap");
      setBootstrap(b);
      if (outcome.published && outcome.withheld.length > 0) {
        // Removed everywhere the app deletes from. On an append-only copy
        // the envelope stays, and it is what lets that key unlock the silo,
        // so calling this revocation would be false.
        toasts.info(
          `Security key removed. Its envelope stays on ${outcome.withheld.join(", ")}, which never deletes anything, so that key still opens the silo for anyone who can read that storage. Changing that means rotating the silo's key.`,
        );
      } else if (outcome.published) {
        toasts.success("Security key removed.");
      } else {
        // Removed here, but the copy in the bucket is what lets that key open
        // the silo from another computer, and it is still there.
        toasts.info(
          "Security key removed on this computer. It can still open this silo elsewhere until the next sync reaches your storage.",
        );
      }
    } catch (e) {
      toasts.error(e);
    } finally {
      end("keys");
    }
  };

  /// What is wrong with this silo, computed once for both the page and the
  /// count on its tab. Waits for everything it judges to have been read:
  /// analysing half-loaded state reports problems that are not there, and a
  /// badge that corrects itself a second later is worse than a late one.
  const healthFindings = useMemo(
    () =>
      passwordsLoaded && siloFactsLoaded
        ? analyseHealth(passwordEntries, {
            backupConfigured: sync.configured,
            securityKeyCount: securityKeys.length,
            recoveryCodeSet: recovery.enabled,
            freeBytes: diskSpace?.available_bytes ?? null,
            headroomBytes: diskSpace?.headroom_bytes ?? 0,
            hostedUsage: hostedUsage
              ? {
                  usedBytes: hostedUsage.account_bytes,
                  quotaBytes: hostedUsage.quota_bytes,
                }
              : null,
            hostedStatus: hostedUsage?.status ?? null,
          })
        : [],
    [
      passwordsLoaded,
      siloFactsLoaded,
      passwordEntries,
      sync.configured,
      securityKeys.length,
      recovery.enabled,
      diskSpace?.available_bytes,
      diskSpace?.headroom_bytes,
      hostedUsage,
    ],
  );

  /// Only what asks to be acted on. The informational findings (duplicates,
  /// a login without a second factor) are worth a line on the page and not
  /// worth a number that never goes away.
  const healthCount = useMemo(
    () => healthFindings.filter((f) => f.severity !== "info").length,
    [healthFindings],
  );

  /// The right-hand end of the status bar. Names what the current view is
  /// showing, which is what turns a lone sync label into a status bar.
  const statusSummary = useMemo(() => {
    if (view === "files") {
      if (!currentFolder) return undefined;
      const folders = entries.filter((e) => e.kind === "folder").length;
      const files = entries.length - folders;
      const parts: string[] = [];
      if (folders > 0) parts.push(`${folders} folder${folders === 1 ? "" : "s"}`);
      if (files > 0) parts.push(`${files} file${files === 1 ? "" : "s"}`);
      if (parts.length === 0) parts.push("Empty folder");
      if (selectedIds.size > 0) parts.push(`${selectedIds.size} selected`);
      return parts.join(" · ");
    }
    if (view === "trash") {
      return trashEntries.length === 1 ? "1 item" : `${trashEntries.length} items`;
    }
    if (view === "passwords") {
      return passwordEntries.length === 1 ? "1 item" : `${passwordEntries.length} items`;
    }
    return undefined;
  }, [view, currentFolder, entries, selectedIds, trashEntries.length, passwordEntries.length]);

  /// The silo-level facts Health judges: which keys can open it, and whether
  /// a recovery code exists. Both used to be read only on entering Settings,
  /// which meant the Health tab counted "no security key is enrolled" and
  /// "no recovery code" against every silo until the user went looking.
  useEffect(() => {
    setSiloFactsLoaded(false);
  }, [meta?.vault_id]);

  useEffect(() => {
    if (!meta || siloFactsLoaded) return;
    void (async () => {
      try {
        setSecurityKeys(await invoke<SecurityKeyInfo[]>("fido_list_keys"));
      } catch {
        setSecurityKeys([]);
      }
      await refreshRecovery();
      setSiloFactsLoaded(true);
    })();
  }, [meta, siloFactsLoaded, refreshRecovery]);

  /// Re-read on entering Health rather than polling: a key added on this
  /// device already refreshes the list, one added on another device would
  /// otherwise never show up here.
  useEffect(() => {
    if (view !== "health" || !meta) return;
    void invoke<SecurityKeyInfo[]>("fido_list_keys")
      .then(setSecurityKeys)
      .catch(() => {});
    void refreshRecovery();
    // Read here as well as at unlock: a disk fills up while the app is open,
    // usually because of something else entirely, and this page is where
    // someone comes to ask what is wrong.
    void refreshDiskSpace();
  }, [view, meta, refreshRecovery, refreshDiskSpace]);

  useEffect(() => {
    if (view !== "passwords") setFocusEntryId(null);
  }, [view]);

  /// Favourites are read on entering the view, and again after any starring,
  /// so a star set on another device shows up the next time the list is
  /// looked at rather than only after a restart.
  useEffect(() => {
    if (view !== "favorites" || !meta) return;
    void refreshFavorites();
  }, [view, meta, refreshFavorites]);

  const crumbs = useMemo(
    () =>
      currentFolder
        ? breadcrumbSegments(currentFolder.path, bootstrap?.silo?.name ?? "Silo")
        : [],
    [currentFolder, bootstrap?.silo?.name],
  );
  const canGoBack = navIndex > 0;
  const canGoForward = navIndex < navHistory.length - 1;
  const canGoUp = Boolean(currentFolder?.parent_id);
  const singleSelected =
    selectedEntries.length === 1 ? selectedEntries[0]! : null;

  useEffect(() => {
    if (!meta || view !== "files") return;
    const onKeyDown = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT" ||
          target.isContentEditable)
      ) {
        return;
      }
      if (e.key === "Backspace" || (e.altKey && e.key === "ArrowUp")) {
        e.preventDefault();
        void goUp();
        return;
      }
      if (e.altKey && e.key === "ArrowLeft") {
        e.preventDefault();
        void goBack();
        return;
      }
      if (e.altKey && e.key === "ArrowRight") {
        e.preventDefault();
        void goForward();
        return;
      }
      if (e.key === "Enter" && singleSelected) {
        e.preventDefault();
        // The same double-click contract: a folder opens here, a file opens
        // in whatever the system uses for it.
        if (singleSelected.kind === "folder") void openFolder(singleSelected);
        else void openFile(singleSelected);
        return;
      }
      if (e.key === "Delete" && selectedEntries.length > 0) {
        e.preventDefault();
        void handleTrash();
        return;
      }
      if (e.key === "F2" && selectedEntries.length === 1) {
        e.preventDefault();
        startRename();
        return;
      }
      if (e.key === "F5") {
        e.preventDefault();
        void refreshCurrentFolder();
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "v" && !busy) {
        e.preventDefault();
        void handlePasteFiles();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- handlers stable enough for session
  }, [
    meta,
    view,
    goUp,
    goBack,
    goForward,
    openFolder,
    singleSelected,
    selectedEntries,
    refreshCurrentFolder,
    busy,
    handlePasteFiles,
  ]);

  // The background pass runs on its own every two minutes; this only picks
  // up what it did, plus any local edits queued since.
  useEffect(() => {
    if (!meta) return;
    void refreshSync();
    const timer = setInterval(() => void refreshSync(), 20_000);
    return () => clearInterval(timer);
  }, [meta, refreshSync]);

  useEffect(() => {
    if (view !== "settings" || !meta) return;
    void invoke<SecurityKeyInfo[]>("fido_list_keys")
      .then(setSecurityKeys)
      .catch(() => setSecurityKeys([]));
    void refreshRecovery();
    void refreshDevices();
  }, [view, meta, refreshRecovery, refreshDevices]);

  /// Entries in memory belong to the silo they were read from, so opening
  /// another one drops them. Without this the panel loaded once per app
  /// launch: a second silo showed the first one's logins, and saving an
  /// edit would have written one silo's entry into another.
  useEffect(() => {
    setPasswordEntries([]);
    setPasswordCategories(null);
    setPasswordsLoaded(false);
  }, [meta?.vault_id]);

  /// Read once the silo is open rather than when Credentials is first
  /// visited. Three things depend on these rows now: the panel, Favourites,
  /// and the count on the Health tab, and a badge that only becomes correct
  /// after the user happens to open a page is worse than no badge.
  useEffect(() => {
    if (!meta || passwordsLoaded) return;
    void (async () => {
      try {
        const json = await invoke<string>("vault_read_passwords");
        // The store holds one reserved row alongside the logins: the
        // category list. Partitioned here so the panel's entry list never
        // sees it.
        const rows = JSON.parse(json) as unknown[];
        const meta_row = rows.find(isCategoriesRow);
        setPasswordCategories(meta_row ? meta_row.categories : null);
        setPasswordEntries(rows.filter((r) => !isCategoriesRow(r)) as PasswordEntry[]);
        setPasswordsLoaded(true);
      } catch (e) {
        // Never quietly: a store that refuses to open looks exactly like an
        // empty one, and for a password manager those are opposite answers.
        toasts.error(e);
        setPasswordEntries([]);
        setPasswordCategories(null);
        setPasswordsLoaded(true);
      }
    })();
  }, [meta, passwordsLoaded, toasts]);

  /// The whole list as one row, unlike entries, which save one at a time.
  /// The list is small and edited rarely, and per-name rows would leave
  /// no place to record an order.
  const savePasswordCategories = useCallback(
    async (categories: PasswordCategory[]) => {
      setPasswordCategories(categories);
      begin("entries");
      try {
        await invoke("vault_upsert_password", {
          id: CATEGORIES_ROW_ID,
          json: JSON.stringify({
            id: CATEGORIES_ROW_ID,
            type: "meta:categories",
            categories,
          }),
        });
      } catch (e) {
        toasts.error(e);
      } finally {
        end("entries");
      }
    },
    [begin, end, toasts],
  );

  /// One entry at a time, because that is what the user changed: rewriting
  /// the whole store meant two devices could not both add a login offline,
  /// the later writer replacing the other's work wholesale. Silent on
  /// success, or a toast per edit trains the user to ignore toasts.
  const savePasswordEntry = useCallback(
    async (entry: PasswordEntry) => {
      setPasswordEntries((prev) => {
        const idx = prev.findIndex((e) => e.id === entry.id);
        if (idx < 0) return [...prev, entry];
        const next = [...prev];
        next[idx] = entry;
        return next;
      });
      begin("entries");
      try {
        await invoke("vault_upsert_password", {
          id: entry.id,
          json: JSON.stringify(entry),
        });
      } catch (e) {
        toasts.error(e);
      } finally {
        end("entries");
      }
    },
    [begin, end, toasts],
  );

  const deletePasswordEntry = useCallback(
    async (id: string) => {
      // The entry is the only thing referencing its attachment blobs, so
      // deleting it is the last chance to reclaim them. Best-effort: a blob
      // that fails to delete is orphaned ciphertext, not a data leak.
      const attachments = passwordEntries.find((e) => e.id === id)?.attachments ?? [];
      setPasswordEntries((prev) => prev.filter((e) => e.id !== id));
      begin("entries");
      try {
        await invoke("vault_delete_password", { id });
        for (const attachment of attachments) {
          void invoke("password_delete_attachment", { blobId: attachment.blob_id }).catch(
            () => {},
          );
        }
      } catch (e) {
        toasts.error(e);
      } finally {
        end("entries");
      }
    },
    [begin, end, passwordEntries, toasts],
  );

  /// Import writes many entries; each is its own operation, so a failure
  /// part way through leaves the ones already stored rather than nothing.
  const importPasswordEntries = useCallback(
    async (entries: PasswordEntry[]) => {
      begin("entries");
      try {
        for (const entry of entries) {
          await invoke("vault_upsert_password", {
            id: entry.id,
            json: JSON.stringify(entry),
          });
          setPasswordEntries((prev) => [...prev, entry]);
        }
      } catch (e) {
        toasts.error(e);
      } finally {
        end("entries");
      }
    },
    [begin, end, toasts],
  );

  const toastHost = <ToastHost toasts={toasts.toasts} onDismiss={toasts.dismiss} />;
  // Rendered alongside the confirmation host, so it reaches whichever screen
  // the user is on when a background pass discovers it.
  const rebuildHost = needsRebuild && (
    <ConfirmDialog
      title="This device is out of step"
      message={
        "It has been away long enough that the changes it missed are no longer stored. " +
        "Setting it up again from the current state takes a moment and loses nothing that " +
        "reached the backup. Anything changed here since then, and never sent, cannot be kept."
      }
      confirmLabel="Set up again"
      cancelLabel="Not now"
      busy={rebuilding}
      onConfirm={() => void rebuildFromSnapshot()}
      onCancel={() => setNeedsRebuild(false)}
    />
  );
  const confirmHost = confirmDialog && (
    <ConfirmDialog
      title={confirmDialog.title}
      message={confirmDialog.message}
      confirmLabel={confirmDialog.confirmLabel}
      danger={confirmDialog.danger}
      option={confirmDialog.option}
      onConfirm={(option) => {
        confirmDialog.resolve({ ok: true, option });
        setConfirmDialog(null);
      }}
      onCancel={() => {
        confirmDialog.resolve({ ok: false, option: false });
        setConfirmDialog(null);
      }}
    />
  );

  if (!bootstrap) {
    return (
      <AuthShell title="SilentSilo" subtitle="Loading…">
        {toastHost}
        {confirmHost}
        {rebuildHost}
        <p className="hint">
          <span className="spinner" aria-hidden /> Starting…
        </p>
      </AuthShell>
    );
  }

  const unlocked =
    bootstrap.provisioned && !bootstrap.locked && meta !== null && bootstrap.fido_enrolled;
  const needsEnrollment = bootstrap.provisioned && !bootstrap.fido_enrolled;

  // No silo open — the picker, or the flow for joining one that lives in a
  // bucket. Held back until the list has actually loaded, so the picker
  // doesn't flash "create your first silo" at someone who has three.
  if (!bootstrap.silo) {
    if (!silosLoaded) return null;
    return (
      <>
        {toastHost}
        {confirmHost}
        {rebuildHost}
        {joining ? (
          <JoinView
            busy={busy("silo")}
            onBack={() => setJoining(false)}
            onJoined={joinSilo}
          />
        ) : (
          <SiloPickerView
            silos={silos}
            busy={busy("silo")}
            onOpen={(id) => void openSilo(id)}
            onCreate={(name, location) => void createSilo(name, location)}
            onJoin={() => setJoining(true)}
            onAdded={() => void refreshSilos()}
            onForget={(silo) => void forgetSilo(silo)}
          />
        )}
      </>
    );
  }

  if (needsEnrollment) {
    return (
      <>
        {toastHost}
        {confirmHost}
        {rebuildHost}
        <EnrollView
          bootstrap={bootstrap}
          busy={busy("keys", "silo")}
          fidoProgress={fidoProgress}
          onRetry={() => void retryFidoDetection()}
          onEnroll={(authenticator) => void enrollPrimaryKey(authenticator)}
          onDiscard={() => void discardUnenrolledSilo()}
          onBack={() => void closeSilo()}
        />
      </>
    );
  }

  if (!unlocked) {
    return (
      <>
        {toastHost}
        {confirmHost}
        {rebuildHost}
        <UnlockView
          bootstrap={bootstrap}
          busy={busy("silo", "keys")}
          fidoProgress={fidoProgress}
          onRetry={() => void retryFidoDetection()}
          onUnlock={() => void unlockSilo()}
          onUnlockWithRecovery={(code) => void unlockSilo(code)}
          onSwitchSilo={() => void closeSilo()}
        />
      </>
    );
  }

  return (
    <>
      {toastHost}
      {confirmHost}
      {rebuildHost}
      {dropActive && (
        <div className="drop-overlay" aria-hidden>
          <div className="drop-overlay-card">
            <IconFilePlus size={32} />
            <strong>Drop to add to {currentFolder?.name ?? "this folder"}</strong>
            <span className="hint">Files are encrypted as they land.</span>
          </div>
        </div>
      )}
      {/* One at a time. Both queues can be non-empty at once (a send and a
          save requested from Explorer before the silo was unlocked), and
          mounting both stacked one modal on top of the other, with the one
          underneath still reacting to clicks. The save dialog waits. */}
      {pendingShellUploadPaths ? (
        <ShellUploadDialog
          paths={pendingShellUploadPaths}
          busy={shellUploadBusy}
          onConfirm={(folderId) => void confirmShellUpload(folderId)}
          onCancel={cancelShellUpload}
        />
      ) : (
        pendingShellDownloadTarget && (
          <ShellDownloadDialog
            targetDir={pendingShellDownloadTarget}
            busy={shellDownloadBusy}
            onConfirm={(selection) => void confirmShellDownload(selection)}
            onCancel={cancelShellDownload}
          />
        )
      )}
      <AppShell
        view={view}
        onView={setView}
        onLock={() => void lockSilo()}
        storage={
          blobStatus
            ? {
                localBytes: blobStatus.usage.local_bytes,
                unsyncedBytes: blobStatus.usage.unsynced_bytes,
              }
            : null
        }
        trashCount={trashEntries.length}
        healthCount={healthCount}
        sync={sync}
        onSyncNow={() => void syncNow()}
        onOpenBackup={() => {
          setSettingsSection("backup");
          setView("settings");
        }}
        statusSummary={statusSummary}
        siloName={bootstrap.silo.name}
        onSwitchSilo={() => void closeSilo()}
        title={
          // Every view names itself through its own rail or toolbar, so a
          // page title above one only spends the vertical space it is
          // trying to use.
          undefined
        }
        theme={theme}
        onToggleTheme={() => themeControl?.toggle()}
      >
        {/* A device that joined or recovered holds the whole file tree and
            none of the content, and each file downloads when it is first
            opened. That suits a laptop and does not suit someone who has
            just replaced a machine, so the offer is made once, here, where
            they are looking at the files it is about. */}
        {view === "files" && meta && sync.configured && (blobStatus?.missing.length ?? 0) > 0 &&
          (!contentOfferDismissed || contentFetch) && (
          <div className="content-offer" role="status">
            <div className="content-offer-text">
              <strong>
                {blobStatus!.missing.length === 1
                  ? "1 file is in the backup but not on this computer"
                  : `${blobStatus!.missing.length} files are in the backup but not on this computer`}
              </strong>
              <p>
                {contentFetch
                  ? `Downloading ${contentFetch.done} of ${contentFetch.total}…`
                  : `They open on demand. Download all ${formatBytes(blobStatus!.missing_bytes)} now to have them here offline.`}
              </p>
            </div>
            <div className="content-offer-actions">
              {contentFetch ? (
                <button type="button" className="secondary" onClick={cancelFetchAllContent}>
                  Stop
                </button>
              ) : (
                <>
                  <button type="button" onClick={() => void fetchAllContent()}>
                    Download everything
                  </button>
                  <button type="button" className="secondary" onClick={dismissContentOffer}>
                    Not now
                  </button>
                </>
              )}
            </div>
          </div>
        )}

        {view === "files" && meta && (
          <FilesExplorer
            currentFolder={currentFolder}
            entries={entries}
            crumbs={crumbs}
            selectedIds={selectedIds}
            busy={busy("transfer", "entry")}
            navigating={busy("navigate")}
            progress={uploadProgress}
            onCancelProgress={uploadCancelable ? cancelUpload : undefined}
            progressCancelling={uploadCancelling}
            newFolderName={newFolderName}
            onNewFolderName={setNewFolderName}
            renamingId={renamingId}
            renameValue={renameValue}
            onRenameValue={setRenameValue}
            canGoBack={canGoBack}
            canGoForward={canGoForward}
            canGoUp={canGoUp}
            onBack={() => void goBack()}
            onForward={() => void goForward()}
            onUp={() => void goUp()}
            onJumpPath={(p) => void jumpToPath(p)}
            onRefresh={() => void refreshCurrentFolder()}
            onAddFiles={() => void handleAddFiles()}
            onAddFolder={() => void handleAddFolder()}
            onCreateFolder={() => void handleCreateFolder()}
            onSelectClick={handleSelectClick}
            onSelectIds={(ids) => {
              setSelectedIds(ids);
              setAnchorIndex(null);
            }}
            onOpenFolder={(f) => void openFolder(f)}
            onOpenFile={(file) => void openFile(file)}
            globalResults={globalResults}
            searching={searching}
            onSearch={runSearch}
            onJumpToHit={(hit) => void jumpToHit(hit)}
            onSaveCopy={(f) => void handleSaveCopy(f)}
            onSaveCopies={(files) => void handleSaveCopies(files)}
            onSaveFolder={(f) => void handleSaveFolder(f)}
            onRenameEntry={startRenameEntry}
            onTrashEntry={(entry) => void handleTrashEntry(entry)}
            onToggleFavorite={(entry) => void toggleFavorite(entry)}
            onStartRename={startRename}
            onCommitRename={() => void commitRename()}
            onCancelRename={cancelRename}
            onTrash={() => void handleTrash()}
            onClearSelection={clearSelection}
            syncConfigured={sync.configured}
            localBlobIds={localBlobIds}
            unsyncedBlobIds={unsyncedBlobIds}
          />
        )}

        {view === "passwords" && meta && (
          <PasswordsPanel
            entries={passwordEntries}
            storedCategories={passwordCategories}
            busy={busy("entries")}
            focusEntryId={focusEntryId}
            onSaveEntry={(entry) => void savePasswordEntry(entry)}
            onDeleteEntry={(id) => void deletePasswordEntry(id)}
            onImportEntries={(entries) => void importPasswordEntries(entries)}
            onSaveCategories={(categories) => void savePasswordCategories(categories)}
            onConfirmRun={confirmRun}
          />
        )}

        {view === "favorites" && meta && (
          <FavoritesPanel
            hits={favoriteHits}
            credentials={passwordEntries.filter((e) => e.favorite)}
            busy={busy("entry", "entries", "navigate")}
            onOpenHit={(hit) => {
              setView("files");
              void jumpToHit(hit);
            }}
            onOpenCredential={(id) => {
              setFocusEntryId(id);
              setView("passwords");
            }}
            onUnstarHit={(hit) =>
              void toggleFavorite(
                hit.kind === "folder"
                  ? { ...hit, kind: "folder" }
                  : { ...hit, kind: "file" },
              )
            }
            onUnstarCredential={(entry) =>
              void savePasswordEntry({ ...entry, favorite: false })
            }
          />
        )}

        {view === "health" && meta && (
          <HealthPanel
            findings={healthFindings}
            entryCount={passwordEntries.length}
            onOpenEntry={(id) => {
              setFocusEntryId(id);
              setView("passwords");
            }}
            onOpenFix={(fix) => {
              setSettingsSection(fix === "backup" ? "backup" : fix === "keys" ? "keys" : "recovery");
              setView("settings");
            }}
          />
        )}

        {view === "trash" && meta && (
          <TrashPanel
            entries={trashEntries}
            busy={busy("entry")}
            onRestore={(entry) => void handleRestore(entry)}
            onRestoreMany={(items) => void handleRestoreMany(items)}
            onDeleteForever={(items) => void handleDeleteForever(items)}
            onEmptyTrash={() => void handleEmptyTrash()}
          />
        )}

        {view === "settings" && (
          <SettingsPanel
            section={settingsSection}
            onSection={setSettingsSection}
            backupPanel={
              <BackupPanel
                busy={busy("transfer", "silo")}
                siloName={bootstrap.silo.name}
                lastSyncAt={sync.lastSyncAt}
                onActivity={() => void refreshSync()}
                fullCopy={fullCopy}
                onFullCopy={(on) => void setFullCopyEnabled(on)}
                missingCount={blobStatus?.missing.length ?? 0}
                missingBytes={blobStatus?.missing_bytes ?? 0}
                localBytes={blobStatus?.usage.local_bytes ?? 0}
                contentFetch={contentFetch}
                onFetchAllContent={() => void fetchAllContent()}
                onCancelFetchContent={cancelFetchAllContent}
              />
            }
            copiesPanel={
              sync.configured ? (
                <CopiesPanel
                  busy={busy("transfer", "silo")}
                  fullCopy={fullCopy}
                  onActivity={() => void refreshSync()}
                />
              ) : (
                <div className="panel-section">
                  <h3>Copies</h3>
                  <p className="hint">
                    This page lists every copy of the silo and how far behind each one is. It has
                    nothing to show until backup storage is connected on the Backup page.
                  </p>
                </div>
              )
            }
            verifyPanel={
              sync.configured ? (
                <VerifyPanel busy={busy("transfer", "silo")} />
              ) : (
                <div className="panel-section">
                  <h3>Verification</h3>
                  <p className="hint">
                    Checking a silo against its storage, and rehearsing a recovery from it, both
                    need backup storage connected on the Backup page first.
                  </p>
                </div>
              )
            }
            busy={busy("keys", "silo")}
            autoUpdateEnabled={autoUpdateEnabled}
            onAutoUpdateEnabled={setAutoUpdateEnabled}
            backgroundUpdate={backgroundUpdate}
            securityKeys={securityKeys}
            newKeyLabel={newKeyLabel}
            onNewKeyLabel={(v) => {
              setNewKeyLabel(v);
              setKeyAddSuccess(null);
            }}
            fidoProgress={view === "settings" ? fidoProgress : null}
            keyAddSuccess={keyAddSuccess}
            onAddKey={(authenticator) => void addSecurityKey(authenticator)}
            platformAvailable={bootstrap.platform_authenticator}
            silo={bootstrap.silo}
            onRenameSilo={(name) => void renameSilo(name)}
            onForgetSilo={() => void forgetSilo()}
            onSwitchSilo={() => void closeSilo()}
            recovery={recovery}
            recoveryCode={recoveryCode}
            onGenerateRecovery={() => void generateRecoveryCode()}
            onDisableRecovery={() => void disableRecovery()}
            onCopyRecoveryCode={() => {
              if (recoveryCode) {
                void invoke("copy_secret_to_clipboard", { text: recoveryCode });
                toasts.success("Copied to the clipboard. Write it down as well.");
              }
            }}
            onDismissRecoveryCode={() => setRecoveryCode(null)}
            onRemoveKey={(id) => void removeSecurityKey(id)}
            onRotateKey={(keep) => void rotateVaultKey(keep)}
            rotationPending={rotationPending}
            onResumeRotation={(credential) => void resumeRotation(credential)}
            onRenameKey={(id, label) => void renameSecurityKey(id, label)}
            devices={devices}
            onRenameDevice={(deviceId, label) => void renameDevice(deviceId, label)}
            autoLockMinutes={autoLockMinutes}
            siloAutoLockMinutes={siloAutoLockMinutes}
            onSiloAutoLockMinutes={(minutes) => void setSiloAutoLock(minutes)}
          />
        )}
      </AppShell>
    </>
  );
}
