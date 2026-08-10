import { useEffect, useMemo, useState } from "react";
import { FileText, Folder, MapPin, RotateCcw, Trash2 } from "lucide-react";
import { ViewHeader } from "../components/ViewHeader";
import type { TrashItem } from "../lib/types";
import { formatBytes, formatDate } from "../lib/format";
import { IconSearch } from "../ui/Icons";

type Props = {
  entries: TrashItem[];
  busy: boolean;
  onRestore: (entry: TrashItem) => void;
  /** Restores several at once, reported as one outcome. */
  onRestoreMany: (entries: TrashItem[]) => void;
  /** Permanently deletes the given entries. The caller confirms first. */
  onDeleteForever: (entries: TrashItem[]) => void;
  onEmptyTrash: () => void;
};

/**
 * Trash as a pane view, like Files and Credentials.
 *
 * It used to be a 720px card in the middle of the window with its own
 * heading above the shell's, which meant a deleted folder full of files
 * scrolled in a column narrower than the explorer it came from. The list
 * now fills the pane and scrolls inside it, and search is what makes a
 * long trash usable at all: the thing you are looking for is by definition
 * something you cannot see any more.
 *
 * Each item is its own capsule with a checkbox, because the two things
 * done here are done in batches: restoring what a mistaken delete took,
 * and clearing out what was meant to go. "Empty trash" alone forced the
 * second to be all or nothing.
 */
export function TrashPanel({
  entries,
  busy,
  onRestore,
  onRestoreMany,
  onDeleteForever,
  onEmptyTrash,
}: Props) {
  const [search, setSearch] = useState("");
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    if (!q) return entries;
    // Name and where it came from: two items called "notes.txt" are told
    // apart by the folder they were deleted out of.
    return entries.filter(
      (e) => e.name.toLowerCase().includes(q) || e.original_path.toLowerCase().includes(q)
    );
  }, [entries, search]);

  /// Selections never outlive the rows they point at: an id restored or
  /// purged on this device, or by another one mid-sync, would otherwise
  /// keep counting towards "3 selected" with nothing on screen to match.
  useEffect(() => {
    setSelectedIds((prev) => {
      const alive = new Set(entries.map((e) => e.id));
      const next = new Set([...prev].filter((id) => alive.has(id)));
      return next.size === prev.size ? prev : next;
    });
  }, [entries]);

  const selected = useMemo(
    () => entries.filter((e) => selectedIds.has(e.id)),
    [entries, selectedIds]
  );

  const visibleSelected = filtered.filter((e) => selectedIds.has(e.id)).length;
  const allVisibleSelected = filtered.length > 0 && visibleSelected === filtered.length;

  const toggle = (id: string) =>
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const toggleAllVisible = () =>
    setSelectedIds((prev) => {
      const next = new Set(prev);
      // Only what the search is showing: clearing a selection the user
      // cannot see would be a surprise either way.
      if (allVisibleSelected) filtered.forEach((e) => next.delete(e.id));
      else filtered.forEach((e) => next.add(e.id));
      return next;
    });

  const countLabel = (n: number) => `${n} item${n === 1 ? "" : "s"}`;

  return (
    <div className="trash-view">
      <ViewHeader
        icon={Trash2}
        title="Trash"
        subtitle={
          entries.length === 0
            ? "Deleted files and folders wait here until you delete them for good"
            : `${countLabel(entries.length)}, restorable until deleted for good`
        }
      />
      <div className="view-toolbar">
        <div className="view-search">
          <span className="search-icon">
            <IconSearch size={16} />
          </span>
          <input
            type="text"
            placeholder="Search trash…"
            aria-label="Search trash"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <button
          type="button"
          className="danger"
          disabled={busy || entries.length === 0}
          onClick={onEmptyTrash}
          title="Permanently delete everything in the trash"
        >
          <Trash2 size={15} />
          Empty trash
        </button>
      </div>

      {/* The selection bar replaces nothing and hides when empty: a row of
          disabled bulk actions above an untouched list is noise. */}
      {selected.length > 0 && (
        <div className="trash-selection-bar" role="status">
          <span className="trash-selection-count">{countLabel(selected.length)} selected</span>
          <div className="trash-selection-actions">
            <button
              type="button"
              className="secondary"
              disabled={busy}
              onClick={() => onRestoreMany(selected)}
            >
              <RotateCcw size={14} />
              Restore
            </button>
            <button
              type="button"
              className="danger"
              disabled={busy}
              onClick={() => onDeleteForever(selected)}
            >
              <Trash2 size={14} />
              Delete forever
            </button>
            <button type="button" className="link" onClick={() => setSelectedIds(new Set())}>
              Clear
            </button>
          </div>
        </div>
      )}

      {filtered.length > 0 && (
        <label className="trash-select-all">
          <input
            type="checkbox"
            checked={allVisibleSelected}
            // Some but not all: neither ticked nor blank says it better.
            ref={(el) => {
              if (el) el.indeterminate = visibleSelected > 0 && !allVisibleSelected;
            }}
            onChange={toggleAllVisible}
          />
          <span>{search.trim() ? "Select all matching" : "Select all"}</span>
        </label>
      )}

      <div className="trash-pane">
        {filtered.length === 0 ? (
          <div className="trash-empty-state">
            {entries.length === 0 ? <Trash2 size={28} /> : <IconSearch size={28} />}
            <p className="hint">
              {entries.length === 0 ? "Trash is empty." : "Nothing here matches that."}
            </p>
          </div>
        ) : (
          <ul className="trash-list">
            {filtered.map((entry) => (
              <li
                key={entry.id}
                className={`trash-row${selectedIds.has(entry.id) ? " is-selected" : ""}`}
              >
                <input
                  type="checkbox"
                  className="trash-row-check"
                  checked={selectedIds.has(entry.id)}
                  aria-label={`Select ${entry.name}`}
                  onChange={() => toggle(entry.id)}
                />
                <span className="trash-row-icon">
                  {entry.kind === "folder" ? <Folder size={18} /> : <FileText size={18} />}
                </span>
                <div className="trash-row-main">
                  <span className="trash-row-name">{entry.name}</span>
                  <span className="trash-row-path">
                    <MapPin size={11} />
                    {entry.original_path === "/" ? "Silo root" : entry.original_path}
                  </span>
                </div>
                <span className="trash-row-size">
                  {entry.kind === "file" ? formatBytes(entry.size_bytes) : "—"}
                </span>
                <span className="trash-row-date">{formatDate(entry.updated_at)}</span>
                <div className="trash-row-actions">
                  <button
                    type="button"
                    className="secondary"
                    disabled={busy}
                    onClick={() => onRestore(entry)}
                  >
                    <RotateCcw size={14} />
                    Restore
                  </button>
                  <button
                    type="button"
                    className="danger"
                    disabled={busy}
                    title="Delete permanently"
                    aria-label={`Delete ${entry.name} permanently`}
                    onClick={() => onDeleteForever([entry])}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
