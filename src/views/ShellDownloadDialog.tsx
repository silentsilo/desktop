import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ChevronUp, FileText, Folder, HardDriveDownload } from "lucide-react";
import type { FolderEntry, VaultEntry } from "../lib/types";
import { formatAppError } from "../lib/errors";
import { useModal } from "../hooks/useModal";

type Props = {
  targetDir: string;
  busy: boolean;
  onConfirm: (selected: VaultEntry[]) => void;
  onCancel: () => void;
};

export function ShellDownloadDialog(props: Props) {
  const { targetDir, busy, onConfirm, onCancel } = props;
  const [folder, setFolder] = useState<FolderEntry | null>(null);
  const [entries, setEntries] = useState<VaultEntry[]>([]);
  const [loading, setLoading] = useState(true);
  // Persists across navigation — a Map (not a Set) so the entries picked in
  // an earlier folder are still available by the time "Download here" is
  // clicked, after browsing has moved on to a different folder's listing.
  const [selected, setSelected] = useState<Map<string, VaultEntry>>(new Map());
  /// A listing that failed leaves nothing to pick from. Without this the
  /// dialog sat on "Loading…" for good, with Cancel as the only way out.
  const [error, setError] = useState<string | null>(null);

  const loadFolder = async (folderId: string) => {
    setLoading(true);
    setError(null);
    try {
      const [f, list] = await Promise.all([
        invoke<FolderEntry>("vault_get_folder", { folderId }),
        invoke<VaultEntry[]>("vault_list_folder", { folderId }),
      ]);
      setFolder(f);
      setEntries(list);
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void (async () => {
      try {
        const root = await invoke<FolderEntry>("vault_root_folder");
        await loadFolder(root.id);
      } catch (e) {
        setError(formatAppError(e));
        setLoading(false);
      }
    })();
  }, []);

  const toggle = (entry: VaultEntry) => {
    setSelected((prev) => {
      const next = new Map(prev);
      if (next.has(entry.id)) next.delete(entry.id);
      else next.set(entry.id, entry);
      return next;
    });
  };

  const dirName = targetDir.replace(/[/\\]+$/, "").split(/[/\\]/).pop() || targetDir;
  const currentLabel = folder ? (folder.path === "/" ? "Silo root" : folder.name) : "";
  const cardRef = useModal(busy ? undefined : onCancel);

  return (
    <div className="modal-overlay" onClick={busy ? undefined : onCancel}>
      <div
        ref={cardRef}
        className="modal-card modal-card-wide"
        role="dialog"
        aria-modal="true"
        aria-label="Save from SilentSilo"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-title-row">
          <span className="modal-title-icon is-download">
            <HardDriveDownload size={20} />
          </span>
          <h3 className="modal-title">Save from SilentSilo</h3>
        </div>
        <div className="modal-body">
          <p className="hint">
            Pick what to save into <strong>{dirName}</strong>. The copies written there are not
            encrypted.
          </p>
          <div className="browser-nav">
            <button
              type="button"
              className="explorer-icon-btn"
              disabled={!folder?.parent_id || busy || loading}
              onClick={() => folder?.parent_id && void loadFolder(folder.parent_id)}
              title="Up one level"
            >
              <ChevronUp size={16} />
            </button>
            <span className="browser-current-path">{currentLabel}</span>
          </div>
          <ul className="folder-picker-list">
            {loading ? (
              <li className="browser-empty">Loading…</li>
            ) : error ? (
              <li className="browser-empty is-error">{error}</li>
            ) : entries.length === 0 ? (
              <li className="browser-empty">Empty folder.</li>
            ) : (
              entries.map((entry) => (
                <li key={entry.id}>
                  <div
                    className={`folder-picker-row-static${selected.has(entry.id) ? " is-selected" : ""}`}
                  >
                    <label className="browser-checkbox">
                      <input
                        type="checkbox"
                        checked={selected.has(entry.id)}
                        disabled={busy}
                        onChange={() => toggle(entry)}
                      />
                      {entry.kind === "folder" ? <Folder size={16} /> : <FileText size={16} />}
                      <span>{entry.name}</span>
                    </label>
                    {entry.kind === "folder" && (
                      <button
                        type="button"
                        className="browser-open-btn"
                        disabled={busy}
                        onClick={() => void loadFolder(entry.id)}
                      >
                        Open
                      </button>
                    )}
                  </div>
                </li>
              ))
            )}
          </ul>
        </div>
        <div className="modal-actions">
          <span className="hint" style={{ marginRight: "auto" }}>
            {selected.size > 0
              ? `${selected.size} item${selected.size === 1 ? "" : "s"} selected`
              : ""}
          </span>
          <button type="button" className="secondary" disabled={busy} onClick={onCancel}>
            Cancel
          </button>
          <button
            type="button"
            disabled={busy || selected.size === 0}
            onClick={() => onConfirm(Array.from(selected.values()))}
          >
            {busy ? "Saving…" : "Save here"}
          </button>
        </div>
      </div>
    </div>
  );
}
