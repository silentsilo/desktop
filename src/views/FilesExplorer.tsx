import { useState, useMemo, useEffect, useRef, useCallback } from "react";
import type { MouseEvent } from "react";
import type {
  BreadcrumbSeg,
  FileSyncState,
  FolderEntry,
  SearchHit,
  VaultEntry,
} from "../lib/types";
import { formatBytes, formatDate, formatDay } from "../lib/format";
import { fileIconFor, fileKindOf } from "../lib/fileKinds";
import { computeMarqueeBox, rectIntersectsBox } from "../lib/marquee";
import { ContextMenu, type ContextMenuItem } from "./ContextMenu";
import { SyncBadge } from "./SyncBadge";
import { ArrowUpDown, Check, ChevronDown, Star } from "lucide-react";
import { FileInfoDialog } from "./FileInfoDialog";
import { useModal } from "../hooks/useModal";
import {
  IconBack,
  IconFile,
  IconFilePlus,
  IconFolder,
  IconFolderDown,
  IconFolderPlus,
  IconForward,
  IconInfo,
  IconUp,
  IconSearch,
  IconGrid,
  IconList,
  IconEdit,
  IconDownload,
  IconExternalLink,
  IconPlus,
  IconRefresh,
  IconTrash,
  IconClose,
} from "../ui/Icons";

type SortField = "name" | "size" | "modified";

const SORT_FIELDS: readonly SortField[] = ["name", "size", "modified"] as const;

const SORT_LABELS: Record<SortField, string> = {
  name: "Name",
  size: "Size",
  modified: "Modified",
};

type Props = {
  currentFolder: FolderEntry | null;
  entries: VaultEntry[];
  crumbs: BreadcrumbSeg[];
  selectedIds: Set<string>;
  busy: boolean;
  /** Moving around, which is quick and must not disable the rest of the
   * toolbar the way a transfer does. Kept apart so the two cannot freeze
   * each other. */
  navigating: boolean;
  progress: string | null;
  onCancelProgress?: () => void;
  progressCancelling?: boolean;
  newFolderName: string;
  onNewFolderName: (v: string) => void;
  renamingId: string | null;
  renameValue: string;
  onRenameValue: (v: string) => void;
  canGoBack: boolean;
  canGoForward: boolean;
  canGoUp: boolean;
  onBack: () => void;
  onForward: () => void;
  onUp: () => void;
  onJumpPath: (path: string) => void;
  onRefresh: () => void;
  onAddFiles: () => void;
  onCreateFolder: () => void;
  onSelectClick: (entry: VaultEntry, e: MouseEvent) => void;
  onSelectIds: (ids: Set<string>) => void;
  onOpenFolder: (folder: Extract<VaultEntry, { kind: "folder" }>) => void;
  onSaveCopy: (file: Extract<VaultEntry, { kind: "file" }>) => void;
  onOpenFile: (file: Extract<VaultEntry, { kind: "file" }>) => void;
  /** Results from across the whole silo; null while not searching. */
  globalResults: SearchHit[] | null;
  searching: boolean;
  onSearch: (query: string) => void;
  onJumpToHit: (hit: SearchHit) => void;
  onSaveCopies: (files: Extract<VaultEntry, { kind: "file" }>[]) => void;
  onStartRename: () => void;
  onCommitRename: () => void;
  onCancelRename: () => void;
  onTrash: () => void;
  onClearSelection?: () => void;
  onAddFolder?: () => void;
  onSaveFolder?: (folder: Extract<VaultEntry, { kind: "folder" }>) => void;
  onRenameEntry: (entry: VaultEntry) => void;
  onTrashEntry: (entry: VaultEntry) => void;
  onToggleFavorite: (entry: VaultEntry) => void;
  /** Without backup storage every file is simply here, so no badge is shown. */
  syncConfigured: boolean;
  localBlobIds: Set<string>;
  unsyncedBlobIds: Set<string>;
};

export function FilesExplorer(props: Props) {
  const {
    entries,
    crumbs,
    selectedIds,
    busy,
    navigating,
    progress,
    onCancelProgress,
    progressCancelling,
    newFolderName,
    onNewFolderName,
    renamingId,
    renameValue,
    onRenameValue,
    canGoBack,
    canGoForward,
    canGoUp,
    onBack,
    onForward,
    onUp,
    onJumpPath,
    onRefresh,
    onAddFiles,
    onCreateFolder,
    onSelectClick,
    onSelectIds,
    onOpenFolder,
    onSaveCopy,
    onOpenFile,
    globalResults,
    searching,
    onSearch,
    onJumpToHit,
    onSaveCopies,
    onStartRename,
    onCommitRename,
    onCancelRename,
    onTrash,
    onClearSelection,
    onAddFolder,
    onSaveFolder,
    onRenameEntry,
    onTrashEntry,
    onToggleFavorite,
    syncConfigured,
    localBlobIds,
    unsyncedBlobIds,
  } = props;

  /**
   * Where a file's content is right now.
   *
   * Only meaningful once backup storage exists. Before that every file is
   * on this disk and nowhere else, and a badge saying so on every row would
   * be decoration.
   */
  const syncStateOf = (entry: VaultEntry): FileSyncState | null => {
    if (!syncConfigured || entry.kind !== "file") return null;
    if (unsyncedBlobIds.has(entry.blob_id)) return "pending";
    return localBlobIds.has(entry.blob_id) ? "backed-up" : "remote-only";
  };

  const [searchQuery, setSearchQuery] = useState("");
  const [ctxMenu, setCtxMenu] = useState<{ x: number; y: number; items: ContextMenuItem[] } | null>(
    null,
  );
  const [infoEntry, setInfoEntry] = useState<VaultEntry | null>(null);
  const [sortBy, setSortBy] = useState<SortField | null>(null);
  const [sortOrder, setSortOrder] = useState<"asc" | "desc">("asc");
  const [showSortMenu, setShowSortMenu] = useState(false);
  const [viewType, setViewType] = useState<"list" | "grid">(
    () => (localStorage.getItem("explorer_view_type") as "list" | "grid") ?? "grid"
  );
  useEffect(() => {
    localStorage.setItem("explorer_view_type", viewType);
  }, [viewType]);
  const [isModalOpen, setIsModalOpen] = useState(false);
  const [showAddDropdown, setShowAddDropdown] = useState(false);
  const newFolderRef = useModal(() => setIsModalOpen(false), isModalOpen);

  // Long paths no longer wrap onto a second line (which broke the toolbar's
  // fixed height) — the address bar scrolls horizontally instead, and stays
  // scrolled to the current (rightmost) segment, since that's the one that
  // actually matters after navigating deeper.
  const addressBarRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = addressBarRef.current;
    if (el) el.scrollLeft = el.scrollWidth;
  }, [crumbs]);

  // Rubber-band (marquee) selection: mousedown on empty background starts a
  // drag; entries whose bounding box intersects the dragged rectangle are
  // selected live, matching the click-and-drag gesture from Explorer/Finder.
  // A mousedown with no subsequent movement is just "click empty space to
  // clear the selection" — same code path, since an empty rectangle
  // intersects nothing.
  const itemRefs = useRef(new Map<string, HTMLElement>());
  const dragCleanupRef = useRef<(() => void) | null>(null);
  const [marquee, setMarquee] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(
    null,
  );

  useEffect(() => () => dragCleanupRef.current?.(), []);

  const registerItemRef = useCallback((id: string, el: HTMLElement | null) => {
    if (el) itemRefs.current.set(id, el);
    else itemRefs.current.delete(id);
  }, []);

  const handleBackgroundMouseDown = useCallback(
    (e: MouseEvent) => {
      if (e.button !== 0) return;
      const target = e.target as HTMLElement;
      if (target.closest(".grid-card, tr")) return;

      const startX = e.clientX;
      const startY = e.clientY;
      onSelectIds(new Set());

      const handleMove = (ev: globalThis.MouseEvent) => {
        const box = computeMarqueeBox(startX, startY, ev.clientX, ev.clientY);
        setMarquee(box);
        const ids = new Set<string>();
        itemRefs.current.forEach((el, id) => {
          if (rectIntersectsBox(el.getBoundingClientRect(), box)) {
            ids.add(id);
          }
        });
        onSelectIds(ids);
      };
      const handleUp = () => {
        window.removeEventListener("mousemove", handleMove);
        window.removeEventListener("mouseup", handleUp);
        dragCleanupRef.current = null;
        setMarquee(null);
      };
      window.addEventListener("mousemove", handleMove);
      window.addEventListener("mouseup", handleUp);
      dragCleanupRef.current = handleUp;
    },
    [onSelectIds],
  );

  const handleSort = (field: SortField) => {
    if (sortBy === field) {
      setSortOrder(sortOrder === "asc" ? "desc" : "asc");
    } else {
      setSortBy(field);
      setSortOrder("asc");
    }
  };

  const handleCreate = () => {
    if (newFolderName.trim()) {
      onCreateFolder();
      setIsModalOpen(false);
    }
  };

  const openBackgroundMenu = (e: MouseEvent) => {
    e.preventDefault();
    const items: ContextMenuItem[] = [
      { kind: "action", label: "Add files", icon: <IconFile size={14} />, onClick: onAddFiles, disabled: busy },
    ];
    if (onAddFolder) {
      items.push({
        kind: "action",
        label: "Add folder",
        icon: <IconFolder size={14} />,
        onClick: onAddFolder,
        disabled: busy,
      });
    }
    items.push(
      { kind: "divider" },
      {
        kind: "action",
        label: "New folder",
        icon: <IconFolder size={14} />,
        onClick: () => {
          onNewFolderName("");
          setIsModalOpen(true);
        },
        disabled: busy,
      },
      { kind: "divider" },
      // Sorting reads as a property of the listing, so it belongs on the
      // menu you get by right-clicking the listing itself. The tick marks
      // which field is active; picking it again flips the direction, the
      // same gesture as the toolbar and the column headers.
      ...SORT_FIELDS.map<ContextMenuItem>((field) => ({
        kind: "action",
        label:
          sortBy === field
            ? `Sort by ${SORT_LABELS[field].toLowerCase()} (${sortOrder === "asc" ? "A-Z" : "Z-A"})`
            : `Sort by ${SORT_LABELS[field].toLowerCase()}`,
        icon: sortBy === field ? <Check size={14} /> : <ArrowUpDown size={14} />,
        onClick: () => handleSort(field),
      })),
      ...(sortBy
        ? [
            {
              kind: "action" as const,
              label: "Folder order",
              icon: <IconClose size={14} />,
              onClick: () => setSortBy(null),
            },
          ]
        : []),
      { kind: "divider" },
      {
        kind: "action",
        label: "Refresh",
        icon: <IconRefresh size={14} />,
        onClick: onRefresh,
        disabled: navigating,
      },
    );
    setCtxMenu({ x: e.clientX, y: e.clientY, items });
  };

  /// Same entry in both the file and folder menus, and the label says what
  /// the click will do rather than what the entry currently is.
  const favoriteItem = (entry: VaultEntry): ContextMenuItem => ({
    kind: "action",
    label: entry.favorite ? "Remove from favourites" : "Add to favourites",
    icon: <Star size={14} fill={entry.favorite ? "currentColor" : "none"} />,
    onClick: () => onToggleFavorite(entry),
    disabled: busy,
  });

  const openEntryMenu = (e: MouseEvent, entry: VaultEntry) => {
    e.preventDefault();
    e.stopPropagation();
    // Right-click never changes the left-click selection (and never pops the
    // selection toolbar) — it only does that when the entry is already part
    // of an existing multi-selection, to offer a bulk action consistent with
    // what the toolbar already shows for that selection.
    const keepMultiSelection = selectedIds.has(entry.id) && selectedIds.size > 1;

    const items: ContextMenuItem[] = [];
    if (keepMultiSelection) {
      // Same rule as the selection toolbar: offered only when every selected
      // item is a file, because a folder in the mix needs its own recursive
      // export rather than this batch. Without it the menu offered exactly
      // one thing for a multi-selection, and that thing was destructive.
      const selectedFiles = Array.from(selectedIds)
        .map((id) => entries.find((e) => e.id === id))
        .filter(
          (e): e is Extract<VaultEntry, { kind: "file" }> => e !== undefined && e.kind === "file",
        );
      if (selectedFiles.length > 0 && selectedFiles.length === selectedIds.size) {
        items.push(
          {
            kind: "action",
            label: `Save a copy of ${selectedFiles.length} files…`,
            icon: <IconDownload size={14} />,
            onClick: () => onSaveCopies(selectedFiles),
            disabled: busy,
          },
          { kind: "divider" },
        );
      }
      items.push({
        kind: "action",
        label: `Trash ${selectedIds.size} items`,
        icon: <IconTrash size={14} />,
        danger: true,
        onClick: onTrash,
        disabled: busy,
      });
    } else if (entry.kind === "folder") {
      items.push(
        { kind: "action", label: "Open", icon: <IconFolder size={14} />, onClick: () => onOpenFolder(entry), disabled: busy },
        { kind: "action", label: "Rename", icon: <IconEdit size={14} />, onClick: () => onRenameEntry(entry), disabled: busy },
        favoriteItem(entry),
        { kind: "divider" },
      );
      if (onSaveFolder) {
        items.push({
          kind: "action",
          label: "Save a copy…",
          icon: <IconFolderDown size={14} />,
          onClick: () => onSaveFolder(entry),
          disabled: busy,
        });
      }
      items.push(
        { kind: "action", label: "Info", icon: <IconInfo size={14} />, onClick: () => setInfoEntry(entry) },
        { kind: "divider" },
        {
          kind: "action",
          label: "Trash",
          icon: <IconTrash size={14} />,
          danger: true,
          onClick: () => onTrashEntry(entry),
          disabled: busy,
        },
      );
    } else {
      items.push(
        { kind: "action", label: "Open", icon: <IconExternalLink size={14} />, onClick: () => onOpenFile(entry), disabled: busy },
        { kind: "action", label: "Save a copy…", icon: <IconDownload size={14} />, onClick: () => onSaveCopy(entry), disabled: busy },
        { kind: "action", label: "Rename", icon: <IconEdit size={14} />, onClick: () => onRenameEntry(entry), disabled: busy },
        favoriteItem(entry),
        { kind: "divider" },
        { kind: "action", label: "Info", icon: <IconInfo size={14} />, onClick: () => setInfoEntry(entry) },
        { kind: "divider" },
        {
          kind: "action",
          label: "Trash",
          icon: <IconTrash size={14} />,
          danger: true,
          onClick: () => onTrashEntry(entry),
          disabled: busy,
        },
      );
    }
    setCtxMenu({ x: e.clientX, y: e.clientY, items });
  };

  /// One search, not two. The box used to also filter the current folder on
  /// the client while the query to the silo was in flight, so a single word
  /// produced two different answers a fraction of a second apart: first the
  /// folder with most of its rows hidden (sometimes "No results found"),
  /// then the silo-wide hits replacing the whole view. The silo-wide search
  /// already covers this folder, so the folder is left alone until it
  /// answers.
  const searchActive = searchQuery.trim().length > 0;

  // Sort entries (keeping folders on top, then files)
  const sortedEntries = useMemo(() => {
    if (!sortBy) return entries;

    const folders = entries.filter((e) => e.kind === "folder");
    const files = entries.filter((e) => e.kind === "file");

    const compare = (a: VaultEntry, b: VaultEntry) => {
      let comparison = 0;
      if (sortBy === "name") {
        comparison = a.name.localeCompare(b.name, undefined, { sensitivity: "base", numeric: true });
      } else if (sortBy === "size") {
        const sizeA = a.kind === "file" ? a.size_bytes : 0;
        const sizeB = b.kind === "file" ? b.size_bytes : 0;
        comparison = sizeA - sizeB;
      } else if (sortBy === "modified") {
        const dateA = new Date(a.updated_at).getTime();
        const dateB = new Date(b.updated_at).getTime();
        comparison = dateA - dateB;
      }

      return sortOrder === "asc" ? comparison : -comparison;
    };

    folders.sort(compare);
    files.sort(compare);

    return [...folders, ...files];
  }, [entries, sortBy, sortOrder]);

  // Arrow keys move the selection, the way every file manager's do. In the
  // list, up and down step one row; in the grid, left and right step one
  // card and up and down step one visual row, measured from how many cards
  // actually share the first card's offset rather than guessed from widths.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
      if (searchActive || renamingId !== null || sortedEntries.length === 0) return;
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
      if (!["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", "Home", "End"].includes(e.key)) {
        return;
      }
      // Sideways means nothing in a single-column list.
      if (viewType === "list" && (e.key === "ArrowLeft" || e.key === "ArrowRight")) return;

      let columns = 1;
      if (viewType === "grid") {
        const els = sortedEntries
          .map((entry) => itemRefs.current.get(entry.id))
          .filter((el): el is HTMLElement => Boolean(el));
        if (els.length > 1) {
          const firstTop = els[0]!.offsetTop;
          columns = Math.max(1, els.filter((el) => el.offsetTop === firstTop).length);
        }
      }

      const last = sortedEntries.length - 1;
      const current =
        selectedIds.size > 0
          ? sortedEntries.findIndex((entry) => selectedIds.has(entry.id))
          : -1;
      let next: number;
      if (e.key === "Home") next = 0;
      else if (e.key === "End") next = last;
      else if (current < 0) next = 0;
      else {
        const step =
          e.key === "ArrowDown"
            ? viewType === "grid"
              ? columns
              : 1
            : e.key === "ArrowUp"
              ? viewType === "grid"
                ? -columns
                : -1
              : e.key === "ArrowRight"
                ? 1
                : -1;
        next = Math.min(last, Math.max(0, current + step));
      }

      e.preventDefault();
      const entry = sortedEntries[next]!;
      onSelectIds(new Set([entry.id]));
      itemRefs.current.get(entry.id)?.scrollIntoView({ block: "nearest" });
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [sortedEntries, viewType, searchActive, renamingId, selectedIds, onSelectIds]);

  // Ctrl+A selects everything in the folder on screen. Scoped to this
  // component's lifetime, so it is only active while the file explorer is
  // actually mounted: elsewhere in the app Ctrl+A does nothing special. It
  // does nothing during a search either, where the rows on screen are hits
  // from all over the silo and are not selectable.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== "a") return;
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
      if (searchActive) return;
      e.preventDefault();
      onSelectIds(new Set(entries.map((entry) => entry.id)));
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [entries, onSelectIds, searchActive]);

  return (
    <>
      <div className="explorer-chrome">
        <div className="explorer-nav" role="toolbar" aria-label="Folder navigation">
          <button
            type="button"
            className="explorer-icon-btn"
            title="Back (Alt+Left)"
            disabled={navigating || !canGoBack}
            onClick={onBack}
          >
            <IconBack />
          </button>
          <button
            type="button"
            className="explorer-icon-btn"
            title="Forward (Alt+Right)"
            disabled={navigating || !canGoForward}
            onClick={onForward}
          >
            <IconForward />
          </button>
          <button
            type="button"
            className="explorer-icon-btn"
            title="Up (Alt+Up / Backspace)"
            disabled={navigating || !canGoUp}
            onClick={onUp}
          >
            <IconUp />
          </button>
          <div className="explorer-address" aria-label="Address bar" ref={addressBarRef}>
            {crumbs.map((seg, i) => {
              const isLast = i === crumbs.length - 1;
              return (
                <span key={seg.path} className="explorer-crumb">
                  {i > 0 && <span className="explorer-sep">/</span>}
                  {isLast ? (
                    <span className="explorer-crumb-current">{seg.label}</span>
                  ) : (
                    <button
                      type="button"
                      className="explorer-crumb-btn"
                      disabled={busy}
                      onClick={() => onJumpPath(seg.path)}
                    >
                      {seg.label}
                    </button>
                  )}
                </span>
              );
            })}
          </div>

          <div className="explorer-search-group">
            <div className="search-input-wrapper">
              <span className="search-icon"><IconSearch size={16} /></span>
              <input
                type="text"
                placeholder="Search this silo…"
                value={searchQuery}
                onChange={(e) => {
                  setSearchQuery(e.target.value);
                  onSearch(e.target.value);
                }}
              />
              {searchQuery && (
                <button
                  type="button"
                  className="search-clear"
                  aria-label="Clear search"
                  onClick={() => {
                    setSearchQuery("");
                    onSearch("");
                  }}
                >
                  <IconClose size={14} />
                </button>
              )}
            </div>
          </div>

          <div className="add-menu-container">
            <button
              type="button"
              className="btn-add-new"
              disabled={busy}
              onClick={() => setShowAddDropdown(!showAddDropdown)}
              aria-haspopup="menu"
              aria-expanded={showAddDropdown}
              title="Add or create"
            >
              <IconPlus size={15} />
              <span className="btn-add-new-label">Add</span>
              <ChevronDown className="chevron-icon" size={13} aria-hidden />
            </button>
            {showAddDropdown && (
              <>
                <div className="dropdown-overlay" onClick={() => setShowAddDropdown(false)} />
                <div className="add-dropdown-menu" role="menu">
                  <button
                    type="button"
                    className="dropdown-item"
                    role="menuitem"
                    onClick={() => {
                      setShowAddDropdown(false);
                      onAddFiles();
                    }}
                  >
                    <IconFilePlus size={16} />
                    <span>Add files</span>
                  </button>
                  <button
                    type="button"
                    className="dropdown-item"
                    role="menuitem"
                    onClick={() => {
                      setShowAddDropdown(false);
                      onAddFolder?.();
                    }}
                  >
                    <IconFolderPlus size={16} />
                    <span>Add folder</span>
                  </button>
                  <div className="dropdown-divider" />
                  <button
                    type="button"
                    className="dropdown-item"
                    role="menuitem"
                    onClick={() => {
                      setShowAddDropdown(false);
                      onNewFolderName("");
                      setIsModalOpen(true);
                    }}
                  >
                    <IconFolder size={16} />
                    <span>New folder</span>
                  </button>
                </div>
              </>
            )}
          </div>

          <button
            type="button"
            className="explorer-icon-btn btn-refresh-sync"
            disabled={navigating}
            onClick={onRefresh}
            title="Refresh (F5)"
            aria-label="Refresh"
          >
            <IconRefresh size={16} />
          </button>

          <button
            type="button"
            className={`view-toggle-btn${viewType === "grid" ? " active" : ""}`}
            onClick={() => setViewType(viewType === "list" ? "grid" : "list")}
            title={viewType === "list" ? "Switch to grid view" : "Switch to list view"}
            aria-label={viewType === "list" ? "Switch to grid view" : "Switch to list view"}
          >
            {viewType === "list" ? <IconGrid size={18} /> : <IconList size={18} />}
          </button>

          {/* Sorting belongs to the listing, not to the table that happens to
              render it, so it lives on the toolbar where both views can reach
              it, and in the right-click menu for the same reason. */}
          <div className="sort-menu-wrap">
            <button
              type="button"
              className={`view-toggle-btn${sortBy ? " active" : ""}`}
              onClick={() => setShowSortMenu((v) => !v)}
              title={sortBy ? `Sorted by ${SORT_LABELS[sortBy]}` : "Sort"}
              aria-label="Sort"
              aria-haspopup="menu"
              aria-expanded={showSortMenu}
            >
              <ArrowUpDown size={17} />
            </button>
            {showSortMenu && (
              <>
                <div className="dropdown-overlay" onClick={() => setShowSortMenu(false)} />
                <div className="add-dropdown-menu sort-dropdown-menu" role="menu">
                  {SORT_FIELDS.map((field) => (
                    <button
                      key={field}
                      type="button"
                      className="dropdown-item"
                      role="menuitemradio"
                      aria-checked={sortBy === field}
                      onClick={() => {
                        handleSort(field);
                        setShowSortMenu(false);
                      }}
                    >
                      {sortBy === field ? <Check size={15} /> : <span className="dropdown-tick" />}
                      <span>{SORT_LABELS[field]}</span>
                      {sortBy === field && (
                        <span className="dropdown-hint">{sortOrder === "asc" ? "A-Z" : "Z-A"}</span>
                      )}
                    </button>
                  ))}
                  {sortBy && (
                    <>
                      <div className="dropdown-divider" />
                      <button
                        type="button"
                        className="dropdown-item"
                        onClick={() => {
                          setSortBy(null);
                          setShowSortMenu(false);
                        }}
                      >
                        <IconClose size={15} />
                        <span>Folder order</span>
                      </button>
                    </>
                  )}
                </div>
              </>
            )}
          </div>
        </div>
        {progress && (
          <div className="upload-progress" role="status">
            <span className="spinner" aria-hidden />
            {progress}
            {onCancelProgress && (
              <button
                type="button"
                className="link upload-cancel-btn"
                disabled={progressCancelling}
                onClick={onCancelProgress}
              >
                {progressCancelling ? "Cancelling…" : "Cancel"}
              </button>
            )}
          </div>
        )}
      </div>

      <section
        className={`file-list${selectedIds.size > 0 ? " has-selection" : ""}`}
        onContextMenu={searchActive ? undefined : openBackgroundMenu}
        onMouseDown={searchActive ? undefined : handleBackgroundMouseDown}
      >
        {searchActive ? (
          // Searching replaces the folder view entirely rather than filtering
          // it: results come from all over the silo, so the breadcrumb above
          // no longer describes what is on screen, and the path on each row is
          // the only thing that says where a hit actually lives.
          // Results already on screen are dimmed rather than cleared while a
          // longer query is still running: emptying the list on every
          // keystroke made typing look like the search kept failing.
          <div className={`search-results${searching ? " is-stale" : ""}`}>
            {globalResults === null ? (
              <p className="hint">Searching…</p>
            ) : globalResults.length === 0 ? (
              <div className="empty-state">
                <p className="empty-title">Nothing matches “{searchQuery}”</p>
                <p className="hint">
                  Search looks at names only. File contents stay encrypted and are never
                  indexed.
                </p>
              </div>
            ) : (
              <ul className="search-result-list">
                {globalResults.map((hit) => (
                  <li key={hit.id}>
                    <button
                      type="button"
                      className="search-result"
                      // The search ends here: leaving the query in place would
                      // filter the folder we just landed in by a term that has
                      // nothing to do with it, hiding the hit's own siblings.
                      onClick={() => {
                        setSearchQuery("");
                        onSearch("");
                        onJumpToHit(hit);
                      }}
                      onDoubleClick={() => {
                        if (hit.kind === "file") onOpenFile(hit);
                      }}
                    >
                      <span className={hit.kind === "folder" ? "row-folder" : "row-file"}>
                        {hit.kind === "folder" ? <IconFolder /> : <IconFile />}
                      </span>
                      <span className="search-result-text">
                        <strong>{hit.name}</strong>
                        <span className="hint">{hit.folder_path}</span>
                      </span>
                      {hit.kind === "file" && (
                        <span className="hint search-result-size">
                          {formatBytes(hit.size_bytes)}
                        </span>
                      )}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        ) : entries.length === 0 ? (
          <div className="empty-state">
            <p className="empty-title">This folder is empty</p>
            <p className="hint">
              Add files from this computer, or drag them onto the window. They are encrypted as
              they land.
            </p>
            <div className="actions">
              <button type="button" disabled={busy} onClick={onAddFiles}>
                <IconFilePlus size={15} />
                Add files
              </button>
              <button
                type="button"
                className="secondary"
                disabled={busy}
                onClick={() => {
                  onNewFolderName("");
                  setIsModalOpen(true);
                }}
              >
                <IconFolder size={15} />
                New folder
              </button>
            </div>
          </div>
        ) : viewType === "grid" ? (
          <>
            <div className="grid-container">
            {sortedEntries.map((entry) => {
              const selected = selectedIds.has(entry.id);
              const renaming = renamingId === entry.id;
              const isFolder = entry.kind === "folder";
              const sizeStr = entry.kind === "file" ? formatBytes(entry.size_bytes) : "";
              const syncState = syncStateOf(entry);
              const FileTypeIcon = fileIconFor(entry.name);

              return (
                <div
                  key={entry.id}
                  ref={(el) => registerItemRef(entry.id, el)}
                  className={`grid-card${selected ? " is-selected" : ""}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    onSelectClick(entry, e);
                  }}
                  onDoubleClick={() => {
                    if (renaming) return;
                    if (entry.kind === "folder") onOpenFolder(entry);
                    else onOpenFile(entry);
                  }}
                  onContextMenu={(e) => openEntryMenu(e, entry)}
                >
                  {syncState && (
                    <span className="grid-card-badge">
                      <SyncBadge state={syncState} compact />
                    </span>
                  )}
                  {entry.favorite && (
                    <span className="grid-card-star" title="In favourites">
                      <Star size={13} fill="currentColor" />
                    </span>
                  )}

                  <div
                    className={`grid-card-icon ${isFolder ? "row-folder" : `row-file kind-${fileKindOf(entry.name)}`}`}
                  >
                    {isFolder ? <IconFolder size={40} /> : <FileTypeIcon size={38} strokeWidth={1.4} />}
                  </div>

                  {renaming ? (
                    <input
                      className="rename-input"
                      autoFocus
                      value={renameValue}
                      onClick={(e) => e.stopPropagation()}
                      onChange={(e) => onRenameValue(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") {
                          e.preventDefault();
                          onCommitRename();
                        }
                        if (e.key === "Escape") {
                          e.preventDefault();
                          onCancelRename();
                        }
                      }}
                      onBlur={() => onCommitRename()}
                    />
                  ) : (
                    <div className="grid-card-name" title={entry.name}>
                      {entry.name || "/"}
                    </div>
                  )}

                  {/* The date the list view gives a column to. Without it,
                      sorting the grid by "Modified" ordered cards by a value
                      nowhere on screen. */}
                  <div className="grid-card-meta">
                    {isFolder ? "Folder" : sizeStr} · {formatDay(entry.updated_at)}
                  </div>

                </div>
              );
            })}
            </div>
          </>
        ) : (
          <table>
            <thead>
              <tr>
                <th onClick={() => handleSort("name")} className="th-sortable">
                  Name
                  {sortBy === "name" && (
                    <span className="sort-indicator">{sortOrder === "asc" ? "▲" : "▼"}</span>
                  )}
                </th>
                <th onClick={() => handleSort("size")} className="th-sortable">
                  Size
                  {sortBy === "size" && (
                    <span className="sort-indicator">{sortOrder === "asc" ? "▲" : "▼"}</span>
                  )}
                </th>
                <th onClick={() => handleSort("modified")} className="th-sortable">
                  Modified
                  {sortBy === "modified" && (
                    <span className="sort-indicator">{sortOrder === "asc" ? "▲" : "▼"}</span>
                  )}
                </th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {sortedEntries.map((entry) => {
                const selected = selectedIds.has(entry.id);
                const renaming = renamingId === entry.id;
                const syncState = syncStateOf(entry);
                const FileTypeIcon = fileIconFor(entry.name);
                return (
                  <tr
                    key={entry.id}
                    ref={(el) => registerItemRef(entry.id, el)}
                    className={`${entry.kind === "folder" ? "row-folder" : "row-file"}${selected ? " is-selected" : ""}`}
                    onClick={(e) => {
                      e.stopPropagation();
                      onSelectClick(entry, e);
                    }}
                    onDoubleClick={() => {
                      if (renaming) return;
                      if (entry.kind === "folder") onOpenFolder(entry);
                      else onOpenFile(entry);
                    }}
                    onContextMenu={(e) => openEntryMenu(e, entry)}
                  >
                    <td>
                      <span className={`entry-name${entry.kind === "file" ? ` kind-${fileKindOf(entry.name)}` : ""}`}>
                        {entry.kind === "folder" ? (
                          <IconFolder />
                        ) : (
                          <FileTypeIcon size={17} strokeWidth={1.6} />
                        )}
                        {renaming ? (
                          <input
                            className="rename-input"
                            autoFocus
                            value={renameValue}
                            onClick={(e) => e.stopPropagation()}
                            onChange={(e) => onRenameValue(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") {
                                e.preventDefault();
                                onCommitRename();
                              }
                              if (e.key === "Escape") {
                                e.preventDefault();
                                onCancelRename();
                              }
                            }}
                            onBlur={() => onCommitRename()}
                          />
                        ) : (
                          <span>{entry.name || "/"}</span>
                        )}
                        {entry.favorite && !renaming && (
                          <span className="row-star" title="In favourites">
                            <Star size={12} fill="currentColor" />
                          </span>
                        )}
                      </span>
                    </td>
                    <td>
                      <span className="cell-size">
                        {entry.kind === "file" ? formatBytes(entry.size_bytes) : "—"}
                        {syncState && <SyncBadge state={syncState} compact />}
                      </span>
                    </td>
                    <td className="col-muted">{formatDate(entry.updated_at)}</td>
                    <td className="row-actions">
                      {entry.kind === "folder" ? (
                        <button
                          type="button"
                          className="link"
                          onClick={(e) => {
                            e.stopPropagation();
                            onOpenFolder(entry);
                          }}
                        >
                          Open
                        </button>
                      ) : (
                        <button
                          type="button"
                          className="link"
                          onClick={(e) => {
                            e.stopPropagation();
                            onSaveCopy(entry);
                          }}
                        >
                          Save a copy
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </section>

      {marquee && (
        <div
          className="drag-select-box"
          style={{
            left: marquee.x0,
            top: marquee.y0,
            width: marquee.x1 - marquee.x0,
            height: marquee.y1 - marquee.y0,
          }}
        />
      )}

      {/* Contextual Floating Selection Toolbar */}
      {selectedIds.size > 0 && (
        <div className="selection-toolbar">
          <span className="selection-toolbar-count">
            {selectedIds.size} {selectedIds.size === 1 ? "item" : "items"} selected
          </span>
          <div className="selection-toolbar-actions">
            <button
              type="button"
              className="selection-toolbar-btn"
              disabled={busy || selectedIds.size !== 1 || renamingId !== null}
              onClick={onStartRename}
            >
              <IconEdit size={14} /> Rename
            </button>

            {(() => {
              const selectedFiles = Array.from(selectedIds)
                .map((id) => entries.find((e) => e.id === id))
                .filter(
                  (e): e is Extract<VaultEntry, { kind: "file" }> =>
                    e !== undefined && e.kind === "file",
                );
              // Only offered when every selected item is a file: a selection
              // mixing in a folder needs the folder's own recursive export,
              // which isn't part of this batch.
              if (selectedFiles.length === 0 || selectedFiles.length !== selectedIds.size) {
                return null;
              }
              return (
                <button
                  type="button"
                  className="selection-toolbar-btn"
                  disabled={busy}
                  onClick={() =>
                    selectedFiles.length === 1
                      ? onSaveCopy(selectedFiles[0]!)
                      : onSaveCopies(selectedFiles)
                  }
                >
                  <IconDownload size={14} /> Save a copy
                  {selectedFiles.length > 1 ? ` (${selectedFiles.length})` : ""}
                </button>
              );
            })()}

            <button
              type="button"
              className="selection-toolbar-btn danger"
              disabled={busy}
              onClick={onTrash}
            >
              <IconTrash size={14} /> Trash
            </button>

            {onClearSelection && (
              <button
                type="button"
                className="selection-toolbar-btn"
                onClick={onClearSelection}
                title="Clear selection"
              >
                <IconClose size={14} /> Clear
              </button>
            )}
          </div>
        </div>
      )}

      {isModalOpen && (
        <div className="modal-overlay" onClick={() => setIsModalOpen(false)}>
          <div
            ref={newFolderRef}
            className="modal-card"
            role="dialog"
            aria-modal="true"
            aria-label="New folder"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="modal-title">New folder</h3>
            <div className="modal-body">
              <input
                type="text"
                placeholder="Folder name"
                aria-label="Folder name"
                value={newFolderName}
                disabled={busy}
                autoFocus
                onChange={(e) => onNewFolderName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    handleCreate();
                  }
                  if (e.key === "Escape") {
                    e.preventDefault();
                    setIsModalOpen(false);
                  }
                }}
              />
            </div>
            <div className="modal-actions">
              <button
                type="button"
                className="secondary"
                disabled={busy}
                onClick={() => setIsModalOpen(false)}
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={busy || !newFolderName.trim()}
                onClick={handleCreate}
              >
                Create
              </button>
            </div>
          </div>
        </div>
      )}

      {ctxMenu && (
        <ContextMenu x={ctxMenu.x} y={ctxMenu.y} items={ctxMenu.items} onClose={() => setCtxMenu(null)} />
      )}

      {infoEntry && (
        <FileInfoDialog
          entry={infoEntry}
          location={props.currentFolder?.path}
          syncState={syncStateOf(infoEntry)}
          onClose={() => setInfoEntry(null)}
        />
      )}
    </>
  );
}

