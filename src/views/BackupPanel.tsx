import { useCallback, useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CheckCircle2,
  CloudUpload,
  HardDriveDownload,
  Laptop,
  LockKeyhole,
  Pencil,
  RefreshCw,
  SearchCheck,
  Server,
  Unplug,
} from "lucide-react";
import type { StoreConfigView } from "../lib/types";
import { formatAppError } from "../lib/errors";
import { formatBytes, formatDay } from "../lib/format";
import { detectPreset } from "../lib/s3Presets";
import { backupHeadline, syncOutcome, type Status, type SyncReport } from "../lib/syncOutcome";
import { ConfirmDialog } from "./ConfirmDialog";
import {
  EMPTY_STORE_DRAFT,
  missingStoreFields,
  StoreConfigForm,
  storeDraftPayload,
  type StoreDraft,
} from "./StoreConfigForm";

/// Kept out of the S3 form's shape on purpose: a folder needs one value and
/// a bucket needs six, and a single struct covering both would make "a
/// folder with an access key" expressible.

const KIND_LABEL: Record<StoreConfigView["kind"], string> = {
  s3: "S3 bucket",
  folder: "Folder",
  "web-dav": "WebDAV",
  sftp: "SFTP",
};

/**
 * The saved connection, shown back without inputs.
 *
 * Read-only on purpose: fields that look editable suggest each one saves on
 * its own, while the backend replaces the connection as a whole. Editing
 * goes through the form, and what is shown here is whatever the backend
 * says it kept.
 */
function StoredSummary({ stored }: { stored: StoreConfigView }) {
  const rows: [string, string][] = [["Type", KIND_LABEL[stored.kind]]];
  if (stored.kind === "s3") {
    rows.push(
      ["Bucket", stored.prefix ? `${stored.bucket}/${stored.prefix}` : stored.bucket],
      ["Endpoint", stored.endpoint],
      ["Region", stored.region],
      ["Access key", stored.access_key_id],
    );
  } else if (stored.kind === "folder") {
    rows.push(["Path", stored.path]);
  } else if (stored.kind === "web-dav") {
    rows.push(["Address", stored.url], ["Username", stored.username]);
  } else {
    rows.push(
      ["Server", `${stored.username}@${stored.host}:${stored.port}`],
      ["Folder", stored.path || "/"],
      ["Sign-in", stored.auth_method === "key" ? "private key" : "password"],
    );
    if (stored.host_fingerprint) rows.push(["Server key", stored.host_fingerprint]);
  }
  return (
    <dl className="backup-config">
      {rows.map(([label, value]) => (
        <div key={label} className="backup-config-row">
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

type Props = {
  busy: boolean;
  /** Unix ms of the last pass that reached the storage, from the app shell. */
  lastSyncAt: number | null;
  /** Lets the shell's status bar refresh right away instead of on its timer. */
  onActivity: () => void;
  /** Content the backup holds that this computer does not. */
  missingCount: number;
  missingBytes: number;
  /**
   * Content this computer holds that the backup does not, whether it is
   * still queued or an upload failed. The headline counted records only,
   * so a silo with every record delivered and a file that never uploaded
   * read as "Everything is backed up".
   */
  unsyncedCount: number;
  unsyncedBytes: number;
  /** Files whose content no backup holds. */
  absentCount?: number;
  /** What the content already here occupies, for the same sentence. */
  localBytes: number;
  /** A download-everything pass in flight, counted in files. */
  contentFetch: { done: number; total: number } | null;
  /** Whether this device is meant to hold every blob, not just the index. */
  fullCopy: boolean;
  onFullCopy: (on: boolean) => void;
  onFetchAllContent: () => void;
  onCancelFetchContent: () => void;
  /** The list of copies, shown under the status once storage is connected. */
  copies?: ReactNode;
  /** Why the last background pass failed, or null when it did not. */
  syncError?: string | null;
  /** Opens the backup test, which has a page of its own. */
  onTestBackup?: () => void;
  /** Unix ms of the last test on this computer, null for never. */
  lastTestedAt?: number | null;
};

/**
 * The backup view: one silo's connection to the storage that backs it up.
 *
 * Grew out of a collapsible section inside Settings. Backup is the only
 * feature here that talks to the outside world, carries the most decisions
 * (four storage kinds, host key verification), and is the difference
 * between a silo that survives this machine and one that does not. That
 * earns a page, not a fold.
 */
export function BackupPanel({
  busy,
  lastSyncAt,
  onActivity,
  missingCount,
  missingBytes,
  unsyncedCount,
  unsyncedBytes,
  absentCount = 0,
  localBytes,
  contentFetch,
  fullCopy,
  onFullCopy,
  onFetchAllContent,
  onCancelFetchContent,
  copies,
  syncError = null,
  onTestBackup,
  lastTestedAt = null,
}: Props) {
  const [connected, setConnected] = useState(false);
  const [status, setStatus] = useState<Status>({ kind: "idle" });
  const [expanded, setExpanded] = useState(false);
  /// Whether the Disconnect question is on screen. One click used to do it,
  /// and a slipped click on a red button silently ended the backup.
  const [confirmingDisconnect, setConfirmingDisconnect] = useState(false);
  const [pending, setPending] = useState(0);
  const [draft, setDraft] = useState<StoreDraft>(EMPTY_STORE_DRAFT);
  /// The saved connection as the backend reports it, for the read-only
  /// summary. There is exactly one per silo: saving replaces it wholesale,
  /// which is what keeps "a folder and a bucket both active" impossible.
  const [stored, setStored] = useState<StoreConfigView | null>(null);
  /// A short phrase naming where the backup goes, for the headline.
  const [where, setWhere] = useState("");

  const payload = () => storeDraftPayload(draft);

  const load = useCallback(async () => {
    try {
      const stored = await invoke<StoreConfigView | null>("s3_get_config");
      setStored(stored);
      if (!stored) {
        setConnected(false);
        return;
      }
      setConnected(true);
      // Before the branches, not inside the S3 one. Asking only there left
      // a folder, a WebDAV share or an SFTP server reading "Everything is
      // backed up" with records still queued, because `pending` never moved
      // off its initial zero.
      try {
        const s = await invoke<{ pending_ops: number }>("sync_status");
        setPending(s.pending_ops);
      } catch {
        setPending(0);
      }
      if (stored.kind === "folder") {
        setDraft((prev) => ({ ...prev, kind: "folder", folder: stored.path }));
        setWhere(stored.path);
        return;
      }
      if (stored.kind === "web-dav") {
        setDraft((prev) => ({
          ...prev,
          kind: "web-dav",
          dav: { url: stored.url, username: stored.username, password: "" },
        }));
        setWhere(stored.url);
        return;
      }
      if (stored.kind === "sftp") {
        setDraft((prev) => ({
          ...prev,
          kind: "sftp",
          sftp: {
            ...prev.sftp,
            host: stored.host,
            port: String(stored.port),
            username: stored.username,
            path: stored.path,
            method: stored.auth_method === "key" ? "key" : "password",
            // Secrets are never sent back; blank means "unchanged".
            password: "",
            privateKey: "",
            passphrase: "",
            fingerprint: stored.host_fingerprint ?? "",
          },
        }));
        setWhere(`${stored.username}@${stored.host}`);
        return;
      }
      setWhere(stored.prefix ? `${stored.bucket}/${stored.prefix}` : stored.bucket);
      setDraft((prev) => ({
        ...prev,
        kind: "s3",
        preset: detectPreset(stored.endpoint),
        s3: {
          endpoint: stored.endpoint,
          region: stored.region,
          bucket: stored.bucket,
          prefix: stored.prefix,
          accessKeyId: stored.access_key_id,
          secretAccessKey: "",
          pathStyle: stored.path_style,
        },
      }));
    } catch {
      setConnected(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const guard = (): boolean => {
    const missing = missingStoreFields(draft, connected);
    if (missing.length > 0) {
      setStatus({ kind: "error", message: `Still needed: ${missing.join(", ")}.` });
      return false;
    }
    return true;
  };

  const handleTest = async () => {
    if (!guard()) return;
    setStatus({ kind: "busy", message: "Writing a test file…" });
    try {
      await invoke("s3_test_config", { config: payload() });
      setStatus({ kind: "ok", message: "Connected. The backup storage is writable." });
    } catch (e) {
      setStatus({ kind: "error", message: formatAppError(e) });
    }
  };

  const handleSave = async () => {
    if (!guard()) return;
    setStatus({ kind: "busy", message: "Verifying and saving…" });
    try {
      await invoke("s3_save_config", { config: payload() });
      setConnected(true);
      setExpanded(false);
      setDraft((prev) => ({
        ...prev,
        s3: { ...prev.s3, secretAccessKey: "" },
        dav: { ...prev.dav, password: "" },
      }));
      setStatus({ kind: "ok", message: "Backup storage connected." });
      void load();
      onActivity();
    } catch (e) {
      setStatus({ kind: "error", message: formatAppError(e) });
    }
  };

  const handleSyncNow = async () => {
    setStatus({ kind: "busy", message: "Syncing…" });
    try {
      const report = await invoke<SyncReport>("sync_now");
      // Renames happen when another device claimed a name first. Surfacing
      // them matters more than the counts: a file the user knows by name is
      // now called something else.
      const renamed =
        report.renamed.length > 0 ? ` Renamed to avoid clashes: ${report.renamed.join(", ")}.` : "";
      setStatus(syncOutcome(report, renamed));
      const s = await invoke<{ pending_ops: number }>("sync_status");
      setPending(s.pending_ops);
      onActivity();
    } catch (e) {
      setStatus({ kind: "error", message: formatAppError(e) });
    }
  };

  const handleDisconnect = async () => {
    setStatus({ kind: "busy", message: "Disconnecting…" });
    try {
      await invoke("s3_disconnect");
      setConnected(false);
      setStored(null);
      setDraft(EMPTY_STORE_DRAFT);
      setWhere("");
      setStatus({ kind: "ok", message: "Disconnected. Nothing in backup storage was deleted." });
      onActivity();
    } catch (e) {
      setStatus({ kind: "error", message: formatAppError(e) });
    }
  };

  const working = busy || status.kind === "busy";

  return (
    <div className="panel backup-panel">
      {/* The state of this silo's backup, said once and plainly. Everything
          else on the page hangs off whether this card says connected. */}
      <div className={`panel-section backup-status${connected ? " is-connected" : ""}`}>
        <div className="backup-status-head">
          <span className="backup-status-icon" aria-hidden>
            <CloudUpload size={22} />
          </span>
          <div className="backup-status-text">
            {connected ? (
              <>
                <h3>Backing up to {where}</h3>
                {/* File content is asked about separately from records,
                    because it goes separately: records are pushed first and
                    the blobs follow, so a pass can deliver every record and
                    still leave a file behind. "Everything is backed up" is
                    only true when both queues are empty. */}
                <p>{backupHeadline(pending, unsyncedCount, unsyncedBytes, lastSyncAt)}</p>
                {syncError && status.kind === "idle" && (
                  <p className="hint is-error" role="status">
                    The last sync failed: {formatAppError(syncError)}
                  </p>
                )}
              </>
            ) : (
              <>
                <h3>Not backed up. This silo is only on this computer.</h3>
                <p>
                  If this computer fails, the silo is lost with it. Connect backup storage you
                  control to keep an encrypted copy there.
                </p>
              </>
            )}
          </div>
        </div>

        {status.kind !== "idle" && !expanded && (
          <p
            className={`hint${status.kind === "error" ? " is-error" : ""}${status.kind === "ok" ? " success-msg" : ""}`}
            role="status"
          >
            {status.kind === "ok" && <CheckCircle2 size={14} />}
            {status.message}
          </p>
        )}

        {!expanded && connected && stored && (
          <StoredSummary stored={stored} />
        )}

        {!expanded && connected && (
          <div className="actions">
            <button type="button" disabled={working} onClick={() => void handleSyncNow()}>
              {status.kind === "busy" ? (
                <span className="spinner" aria-hidden />
              ) : (
                <RefreshCw size={15} />
              )}
              {status.kind === "busy" ? "Syncing…" : "Sync now"}
            </button>
            <button type="button" className="secondary" onClick={() => setExpanded(true)}>
              <Pencil size={15} />
              Edit
            </button>
            {onTestBackup && (
              <button type="button" className="secondary" disabled={working} onClick={onTestBackup}>
                <SearchCheck size={15} />
                Test backup
              </button>
            )}
            <button
              type="button"
              className="danger"
              disabled={working}
              onClick={() => setConfirmingDisconnect(true)}
            >
              <Unplug size={15} />
              Disconnect
            </button>
          </div>
        )}

        {!expanded && connected && onTestBackup && (
          <p className="hint">
            {lastTestedAt
              ? `Last tested ${formatDay(Math.floor(lastTestedAt / 1000))} on this computer.`
              : "Never tested from this computer."}
          </p>
        )}
      </div>

      {confirmingDisconnect && (
        <ConfirmDialog
          title="Disconnect this backup?"
          message={`This silo stops backing up to ${where} and to every other copy. Nothing there is deleted, but the silo is only on this computer until you connect backup storage again.`}
          confirmLabel="Disconnect"
          danger
          busy={working}
          onConfirm={() => {
            setConfirmingDisconnect(false);
            void handleDisconnect();
          }}
          onCancel={() => setConfirmingDisconnect(false)}
        />
      )}

      {/* Every copy, the main one included, right under the status: one
          subject, one page. The backup test keeps a page of its own. */}
      {!expanded && connected && copies}

      {/* The other direction. Everything above is about content leaving this
          machine; this is about getting it back, which is the question
          someone asks on the day they replace a computer. */}
      {!expanded && connected && (
        <div className="panel-section">
          <h3>
            <HardDriveDownload size={16} />
            On this computer
          </h3>
          {absentCount > 0 && (
            <p className="hint">
              {absentCount === 1
                ? "1 file is missing: its content is in no backup storage and not on this computer."
                : `${absentCount} files are missing: their content is in no backup storage and not on this computer.`}{" "}
              They show as Missing in Files. A device that still has them uploads them when it syncs.
            </p>
          )}
          {missingCount > 0 ? (
            <>
              <p>
                {missingCount === 1
                  ? "1 file is in backup storage but not here"
                  : `${missingCount} files are in backup storage but not here`}{" "}
                ({formatBytes(missingBytes)}). A computer that was just set up from backup storage
                starts with the file list and downloads each file when you open it.
              </p>
              <div className="actions">
                {contentFetch ? (
                  <>
                    <button type="button" disabled>
                      Downloading {contentFetch.done} of {contentFetch.total}…
                    </button>
                    <button type="button" className="secondary" onClick={onCancelFetchContent}>
                      Stop
                    </button>
                  </>
                ) : (
                  <button type="button" disabled={busy} onClick={onFetchAllContent}>
                    <HardDriveDownload size={15} />
                    Download everything
                  </button>
                )}
              </div>
              <p className="hint">
                Stopping keeps whatever has already arrived. Running it again fetches the rest.
              </p>
            </>
          ) : (
            <p>
              Every file in this silo is on this computer ({formatBytes(localBytes)}), as well as in
              backup storage.
            </p>
          )}

          {/* The setting that decides whether this device counts as a copy at
              all. Under the missing-files line because that line is exactly
              the evidence for it. */}
          <label className="confirm-option full-copy-toggle">
            <input
              type="checkbox"
              checked={fullCopy}
              disabled={busy}
              onChange={(e) => void onFullCopy(e.target.checked)}
            />
            <span>
              Keep a full copy on this computer
              <span className="hint">
                Without this, this computer keeps the file list and downloads each file when you
                open it, so it does not count as a copy. With it, every file is downloaded in the
                background and kept here.
              </span>
            </span>
          </label>
        </div>
      )}

      {(expanded || !connected) && (
        <div className="panel-section">
          <h3>
            <Server size={16} />
            {connected ? "Change backup storage" : "Choose backup storage"}
          </h3>
          <p>
            Any S3-compatible bucket, a folder or network share, WebDAV, or SFTP.
            {connected && " Saving replaces the current backup storage."}
          </p>

          <div className="s3-form">
            <StoreConfigForm
              draft={draft}
              onChange={setDraft}
              hasStoredSecret={connected}
              busy={working}
            />

            {status.kind !== "idle" && (
              <p
                className={`hint${status.kind === "error" ? " is-error" : ""}${status.kind === "ok" ? " success-msg" : ""}`}
                role="status"
              >
                {status.kind === "ok" && <CheckCircle2 size={14} />}
                {status.message}
              </p>
            )}

            <div className="actions">
              <button type="button" disabled={working} onClick={() => void handleSave()}>
                {status.kind === "busy" && <span className="spinner" aria-hidden />}
                {status.kind === "busy" ? "Working…" : "Save and connect"}
              </button>
              <button
                type="button"
                className="secondary"
                disabled={working}
                onClick={() => void handleTest()}
              >
                Test connection
              </button>
              {connected && (
                <button
                  type="button"
                  className="secondary"
                  disabled={working}
                  onClick={() => {
                    setStatus({ kind: "idle" });
                    setExpanded(false);
                  }}
                >
                  Cancel
                </button>
              )}
            </div>
          </div>
        </div>
      )}

      {/* Hidden once someone is mid-task: prose beside a form is noise. */}
      {!expanded && (
      <div className="panel-section backup-explainer">
        <h3>
          <LockKeyhole size={16} />
          How it works
        </h3>
        <ul className="backup-points">
          <li>
            <span className="backup-point-icon" aria-hidden>
              <LockKeyhole size={16} />
            </span>
            <div>
              <strong>Encrypted before it leaves.</strong>
              <p>
                Files, names and passwords are encrypted on this computer before they are sent to
                backup storage.
              </p>
            </div>
          </li>
          <li>
            <span className="backup-point-icon" aria-hidden>
              <Server size={16} />
            </span>
            <div>
              <strong>Storage you already own.</strong>
              <p>A bucket, a NAS folder, a Nextcloud, an SFTP account. No SilentSilo server.</p>
            </div>
          </li>
          <li>
            <span className="backup-point-icon" aria-hidden>
              <Laptop size={16} />
            </span>
            <div>
              <strong>It is also sync.</strong>
              <p>
                Point a second computer at the same backup storage and the silo appears there, kept up
                to date in both directions.
              </p>
            </div>
          </li>
          <li>
            <span className="backup-point-icon" aria-hidden>
              <HardDriveDownload size={16} />
            </span>
            <div>
              <strong>It is your way back.</strong>
              <p>
                On a new computer, choose <em>Set up from backup storage</em> and unlock with your
                key or recovery code.
              </p>
            </div>
          </li>
        </ul>
        <p className="hint">Each silo has its own backup storage, so different silos can back up to different places.</p>
      </div>
      )}
    </div>
  );
}
