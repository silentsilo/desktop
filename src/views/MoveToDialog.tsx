import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useModal } from "../hooks/useModal";
import { IconFolder } from "../ui/Icons";
import { formatAppError } from "../lib/errors";
import { canMoveTo, type MoveDestination } from "../lib/moves";
import type { FolderEntry, VaultEntry } from "../lib/types";

type Props = {
  moving: VaultEntry[];
  /** The folder they are in now, which is not offered. */
  currentPath: string;
  rootLabel: string;
  onPick: (destination: MoveDestination) => void;
  onCancel: () => void;
  /** For tests and the mock: where the folders come from. */
  load?: () => Promise<FolderEntry[]>;
};

const loadFolders = () => invoke<FolderEntry[]>("vault_list_all_folders");

/**
 * "Move to…": every folder of the silo as a tree, for a destination too far
 * away to drag to. The folders being moved, and everything inside them, are
 * shown but cannot be picked.
 */
export function MoveToDialog({
  moving,
  currentPath,
  rootLabel,
  onPick,
  onCancel,
  load = loadFolders,
}: Props) {
  const cardRef = useModal(onCancel);
  const [folders, setFolders] = useState<FolderEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [chosen, setChosen] = useState<FolderEntry | null>(null);

  useEffect(() => {
    let cancelled = false;
    load()
      .then((list) => {
        if (!cancelled) setFolders([...list].sort((a, b) => a.path.localeCompare(b.path)));
      })
      .catch((e) => {
        if (!cancelled) setError(formatAppError(e));
      });
    return () => {
      cancelled = true;
    };
  }, [load]);

  const labelOf = (folder: FolderEntry) => (folder.path === "/" ? rootLabel : folder.name);
  const depthOf = (folder: FolderEntry) =>
    folder.path === "/" ? 0 : folder.path.split("/").length - 1;
  const allowed = useMemo(
    () => (folder: FolderEntry) =>
      canMoveTo(moving, { id: folder.id, path: folder.path }, currentPath),
    [moving, currentPath],
  );
  const title = moving.length === 1 ? `Move "${moving[0]!.name}" to…` : `Move ${moving.length} items to…`;

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div
        ref={cardRef}
        className="modal-card move-to"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="modal-title">{title}</h3>
        <div className="modal-body">
          {error && <p className="hint is-error">{error}</p>}
          {!folders && !error && <p className="hint">Reading the folders…</p>}
          {folders && (
            <ul className="move-to-list" role="listbox" aria-label="Folders">
              {folders.map((folder) => {
                const ok = allowed(folder);
                return (
                  <li key={folder.id}>
                    <button
                      type="button"
                      role="option"
                      aria-selected={chosen?.id === folder.id}
                      className={`move-to-row${chosen?.id === folder.id ? " is-chosen" : ""}`}
                      style={{ paddingLeft: `${0.6 + depthOf(folder) * 1.1}rem` }}
                      disabled={!ok}
                      title={ok ? folder.path : undefined}
                      onClick={() => setChosen(folder)}
                      onDoubleClick={() =>
                        ok && onPick({ id: folder.id, path: folder.path, label: labelOf(folder) })
                      }
                    >
                      <IconFolder size={15} />
                      <span>{labelOf(folder)}</span>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
        </div>
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={onCancel}>
            Cancel
          </button>
          <button
            type="button"
            disabled={!chosen}
            onClick={() =>
              chosen && onPick({ id: chosen.id, path: chosen.path, label: labelOf(chosen) })
            }
          >
            Move here
          </button>
        </div>
      </div>
    </div>
  );
}
