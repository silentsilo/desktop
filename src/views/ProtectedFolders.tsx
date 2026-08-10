import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { FolderHeart, RefreshCw, Trash2 } from "lucide-react";
import { formatAppError } from "../lib/errors";

type ProtectedFolder = { path: string; target: string };

/**
 * Folders on this computer the silo keeps a copy of.
 *
 * The wording here is the feature. "Protected folder" reads like a mirror to
 * most people, and this is an archive: content goes in, nothing comes back
 * out on its own, and deleting a file on the computer does not delete the
 * copy. Someone who expects a mirror and gets an archive is only surprised
 * on the day they were counting on the deletion, which is the worst possible
 * day to find out.
 */
export function ProtectedFoldersPanel() {
  const [folders, setFolders] = useState<ProtectedFolder[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setFolders(await invoke<ProtectedFolder[]>("protected_folders_list"));
    } catch (e) {
      setError(formatAppError(e));
      setFolders([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const add = async () => {
    const picked = await openDialog({ directory: true, multiple: false });
    if (typeof picked !== "string") return;
    setError(null);
    setBusy(true);
    try {
      await invoke("protected_folders_add", { path: picked });
      await refresh();
      // Scanned straight away rather than at the next unlock: someone who
      // just added a folder is waiting to see it arrive.
      await scan();
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (path: string) => {
    setError(null);
    setBusy(true);
    try {
      await invoke("protected_folders_remove", { path });
      await refresh();
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setBusy(false);
    }
  };

  const scan = async () => {
    setError(null);
    setStatus(null);
    setBusy(true);
    try {
      const report = await invoke<{ imported: number; skipped: number }>(
        "protected_folders_scan",
      );
      const parts = [
        report.imported === 1 ? "1 file copied in" : `${report.imported} files copied in`,
      ];
      if (report.skipped > 0) parts.push(`${report.skipped} could not be read`);
      setStatus(report.imported === 0 && report.skipped === 0 ? "Nothing new." : parts.join(", "));
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="panel-section">
      <h3>
        <FolderHeart size={16} />
        Protected folders
      </h3>
      <p>
        Folders on this computer that the silo keeps a copy of. They are checked when you unlock
        the silo and whenever you ask. Nothing is ever written back to them.
      </p>
      <p className="hint">
        This is an archive, not a mirror. A file you delete on this computer stays in the silo,
        which is the point of putting a folder here. To remove one from the silo, delete it in
        Files.
      </p>

      {folders !== null && folders.length > 0 && (
        <ul className="key-list">
          {folders.map((folder) => (
            <li key={folder.path} className="key-list-item">
              {/* Two lines, not one: a Windows path and a vault path run
                  together read as a single nonsense string. */}
              <div className="protected-row-text">
                <strong>{folder.path}</strong>
                <span className="hint">Copied into {folder.target}</span>
              </div>
              <button
                type="button"
                className="secondary"
                disabled={busy}
                onClick={() => void remove(folder.path)}
                title="Stop copying this folder"
              >
                <Trash2 size={14} />
                Stop
              </button>
            </li>
          ))}
        </ul>
      )}

      {folders !== null && folders.length === 0 && (
        <p className="hint">No folders are being copied yet.</p>
      )}

      <div className="actions">
        <button type="button" disabled={busy} onClick={() => void add()}>
          <FolderHeart size={15} />
          Add a folder
        </button>
        <button
          type="button"
          className="secondary"
          disabled={busy || folders === null || folders.length === 0}
          onClick={() => void scan()}
        >
          <RefreshCw size={15} />
          Check now
        </button>
      </div>

      {status && <p className="hint">{status}</p>}
      {error && (
        <p className="hint is-error" role="status">
          {error}
        </p>
      )}
    </div>
  );
}
