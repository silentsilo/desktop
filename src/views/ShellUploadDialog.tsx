import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { FilePlus2, Folder, ChevronUp } from "lucide-react";
import type { Bootstrap, FolderEntry, Silo, VaultEntry } from "../lib/types";
import { formatAppError } from "../lib/errors";
import { useModal } from "../hooks/useModal";

type Props = {
  paths: string[];
  busy: boolean;
  onConfirm: (folderId: string) => void;
  onCancel: () => void;
};

/** Silo folders as normally rendered, `folder` VaultEntry variant only. */
type Subfolder = Extract<VaultEntry, { kind: "folder" }>;

export function ShellUploadDialog(props: Props) {
  const { paths, busy, onConfirm, onCancel } = props;
  const [folder, setFolder] = useState<FolderEntry | null>(null);
  const [subfolders, setSubfolders] = useState<Subfolder[]>([]);
  const [loading, setLoading] = useState(true);
  /// Only the unlocked ones. A locked silo is a destination that would stop
  /// to ask for a key halfway through a drop from Explorer, which is not a
  /// choice worth offering here.
  const [openSilos, setOpenSilos] = useState<Silo[]>([]);
  const [siloId, setSiloId] = useState<string | null>(null);
  /// A folder listing that failed leaves nothing to browse. Without this the
  /// dialog sat on "Loading…" for good, with Cancel as the only way out.
  const [error, setError] = useState<string | null>(null);
  /// The silo that was in focus before this dialog opened. Choosing a
  /// different destination here focuses that silo — the folder listing and
  /// the import that follows both act on whichever silo is focused — so
  /// backing out has to put the app back where it was rather than leaving
  /// the user in a silo they never asked to switch to.
  const [siloOnOpen, setSiloOnOpen] = useState<string | null>(null);

  /// Lands in Inbox, falling back to the root if it isn't there.
  const loadInbox = async () => {
    try {
      const root = await invoke<FolderEntry>("vault_root_folder");
      const rootList = await invoke<VaultEntry[]>("vault_list_folder", { folderId: root.id });
      const inbox = rootList.find((e) => e.kind === "folder" && e.name === "Inbox");
      await loadFolder(inbox?.id ?? root.id);
    } catch (e) {
      setError(formatAppError(e));
      setLoading(false);
    }
  };

  const loadFolder = async (folderId: string) => {
    setLoading(true);
    setError(null);
    try {
      const [f, list] = await Promise.all([
        invoke<FolderEntry>("vault_get_folder", { folderId }),
        invoke<VaultEntry[]>("vault_list_folder", { folderId }),
      ]);
      setFolder(f);
      setSubfolders(list.filter((e): e is Subfolder => e.kind === "folder"));
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void (async () => {
      // Lands in Inbox by default, falling back to the root if it isn't there.
      const silos = await invoke<Silo[]>("silo_open_list").catch(() => [] as Silo[]);
      setOpenSilos(silos);
      // The silo already in focus, not the first alphabetically: the folder
      // list below is the focused silo's, so anything else would show one
      // silo's name over another silo's folders.
      const current = await invoke<Bootstrap>("app_bootstrap").catch(() => null);
      setSiloId(current?.silo?.id ?? silos[0]?.id ?? null);
      setSiloOnOpen(current?.silo?.id ?? null);
      await loadInbox();
    })();
    // Runs once on mount. loadInbox is redefined every render, so listing it
    // would reload the folder list in a loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /// Cancelling undoes the focus change a silo switch made, so dismissing
  /// this dialog leaves the app exactly as it found it.
  const handleCancel = () => {
    if (siloOnOpen && siloId && siloOnOpen !== siloId) {
      void invoke("silo_open", { id: siloOnOpen }).catch(() => {});
    }
    onCancel();
  };

  /// Switching silo means focusing it, so the folder list below — and the
  /// import that follows — act on the one the user just named.
  const chooseSilo = async (id: string) => {
    setLoading(true);
    setSiloId(id);
    try {
      await invoke("silo_open", { id });
    } catch (e) {
      setError(formatAppError(e));
      setLoading(false);
      return;
    }
    await loadInbox();
  };

  const currentLabel = folder ? (folder.path === "/" ? "Silo root" : folder.name) : "";
  const siloLabel = openSilos.find((s) => s.id === siloId)?.name ?? "";
  const cardRef = useModal(busy ? undefined : handleCancel);

  return (
    <div className="modal-overlay" onClick={busy ? undefined : handleCancel}>
      <div
        ref={cardRef}
        className="modal-card modal-card-wide"
        role="dialog"
        aria-modal="true"
        aria-label="Add to SilentSilo"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-title-row">
          <span className="modal-title-icon">
            <FilePlus2 size={20} />
          </span>
          <h3 className="modal-title">Add to SilentSilo</h3>
        </div>
        <div className="modal-body">
          <p className="hint">
            {/* "Item" rather than "file": the context menu is offered on
                folders too, and calling a folder a file here was the first
                sign that the path behind this only handled files. */}
            {paths.length === 1
              ? "1 item from Windows Explorer. Choose where it should go."
              : `${paths.length} items from Windows Explorer. Choose where they should go.`}
          </p>
          {/* Only when there is a choice to make. One open silo is not a
              question, and asking it anyway would tax the common case to
              serve the rare one. */}
          {openSilos.length > 1 && (
            <label className="field">
              <span>Silo</span>
              <select
                value={siloId ?? ""}
                disabled={busy || loading}
                onChange={(e) => void chooseSilo(e.target.value)}
              >
                {openSilos.map((silo) => (
                  <option key={silo.id} value={silo.id}>
                    {silo.name}
                  </option>
                ))}
              </select>
            </label>
          )}
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
            ) : subfolders.length === 0 ? (
              <li className="browser-empty">No subfolders here.</li>
            ) : (
              subfolders.map((f) => (
                <li key={f.id}>
                  <button
                    type="button"
                    className="folder-picker-row"
                    disabled={busy}
                    onClick={() => void loadFolder(f.id)}
                  >
                    <Folder size={16} />
                    <span>{f.name}</span>
                  </button>
                </li>
              ))
            )}
          </ul>
        </div>
        <div className="modal-actions">
          <button type="button" className="secondary" disabled={busy} onClick={handleCancel}>
            Cancel
          </button>
          <button
            type="button"
            disabled={busy || !folder || loading}
            onClick={() => folder && onConfirm(folder.id)}
          >
            {busy
              ? "Adding…"
              : !folder
                ? "Add"
                : openSilos.length > 1 && siloLabel
                  ? `Add to “${currentLabel}” in ${siloLabel}`
                  : `Add to “${currentLabel}”`}
          </button>
        </div>
      </div>
    </div>
  );
}
