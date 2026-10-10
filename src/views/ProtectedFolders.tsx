import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useLasting } from "../lib/lasting";
import { open as openDialog } from "../lib/dialog";
import { FolderHeart, RefreshCw, Trash2 } from "lucide-react";
import { formatAppError } from "../lib/errors";
import { t, useLocale } from "../i18n";

type ProtectedFolder = { path: string; target: string };

/**
 * Folders on this computer the silo keeps a copy of.
 *
 * The wording here is the feature. It was called "Protected folders", which
 * said nothing about how it works and read like a mirror to most people. It
 * is one way: content goes in, nothing comes back out on its own, and
 * deleting a file on the computer does not delete the copy. Someone who expects a mirror and gets an archive is only surprised
 * on the day they were counting on the deletion, which is the worst possible
 * day to find out.
 */
const never = () => false;

export function ProtectedFoldersPanel() {
  useLocale();
  const [folders, setFolders] = useState<ProtectedFolder[] | null>(null);
  // Lasting: a scan imports a whole folder and goes on when the page is left.
  const [busy, setBusy] = useLasting<boolean>("protected.busy", false, Boolean);
  const [status, setStatus] = useLasting<string | null>("protected.status", null, never);
  const [error, setError] = useLasting<string | null>("protected.error", null, never);
  const [progress, setProgress] = useState<string | null>(null);

  // Also the scan unlock started, which "Check now" waits for.
  useEffect(() => {
    const stop = listen<{ done: number; total: number }>(
      "protected-scan-progress",
      ({ payload: { done, total } }) => {
        setProgress(total === 0 ? t("set.pf_looking") : t("set.pf_importing", { done, total }));
      },
    );
    return () => {
      void stop.then((off) => off());
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      setFolders(await invoke<ProtectedFolder[]>("protected_folders_list"));
      setError(null);
    } catch (e) {
      // Left null rather than emptied. The list is sealed under the silo's
      // content key, so a locked silo answers with an error and not with
      // nothing: showing "No folders are being copied yet" under it would
      // tell the user they protect nothing.
      setError(formatAppError(e));
      setFolders(null);
    }
  }, [setError]);

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
      const imported = t("set.pf_scan_imported", { count: report.imported });
      setStatus(
        report.imported === 0 && report.skipped === 0
          ? t("set.pf_nothing_new")
          : report.skipped > 0
            ? t("set.pf_scan_skipped", { imported, count: report.skipped })
            : imported,
      );
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  return (
    <div className="panel-section">
      <h3>
        <FolderHeart size={16} />
        {t("settings.protected")}
      </h3>
      <p>{t("set.pf_intro")}</p>
      <p className="hint">{t("set.pf_delete_hint")}</p>

      {folders !== null && folders.length > 0 && (
        <ul className="key-list">
          {folders.map((folder) => (
            <li key={folder.path} className="key-list-item">
              {/* Two lines, not one: a Windows path and a vault path run
                  together read as a single nonsense string. */}
              <div className="protected-row-text">
                <strong>{folder.path}</strong>
                <span className="hint">{t("set.pf_target", { target: folder.target })}</span>
              </div>
              <button
                type="button"
                className="btn-secondary"
                disabled={busy}
                onClick={() => void remove(folder.path)}
                data-tooltip={t("set.pf_stop_tooltip")}
              >
                <Trash2 size={14} />
                {t("set.pf_stop")}
              </button>
            </li>
          ))}
        </ul>
      )}

      {folders !== null && folders.length === 0 && (
        <p className="hint">{t("set.pf_none")}</p>
      )}

      <div className="actions">
        <button className="btn-primary" type="button" disabled={busy} onClick={() => void add()}>
          <FolderHeart size={15} />
          {t("set.pf_add")}
        </button>
        <button
          type="button"
          className="btn-secondary"
          disabled={busy || folders === null || folders.length === 0}
          onClick={() => void scan()}
        >
          <RefreshCw size={15} />
          {t("set.pf_check")}
        </button>
      </div>

      {busy && progress && (
        <p className="hint" role="status">
          {progress}
        </p>
      )}
      {status && <p className="hint">{status}</p>}
      {error && (
        <p className="hint is-error" role="status">
          {error}
        </p>
      )}
    </div>
  );
}
