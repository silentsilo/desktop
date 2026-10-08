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
import { isCloudView, type StoreConfigView } from "../lib/types";
import { CLOUD_NAME } from "../lib/cloud";
import { KDRIVE_DEFAULT_FOLDER, parseKdriveUrl } from "../lib/kdrive";
import { formatAppError } from "../lib/errors";
import { formatBytes, formatDay } from "../lib/format";
import { detectPreset } from "../lib/s3Presets";
import { backupHeadline, syncOutcome, type Status, type SyncReport } from "../lib/syncOutcome";
import { ConfirmDialog } from "./ConfirmDialog";
import { useLasting } from "../lib/lasting";
import { t, tx, useLocale } from "../i18n";
import {
  discardSignIns,
  EMPTY_STORE_DRAFT,
  missingStoreFields,
  StoreConfigForm,
  storeDraftPayload,
  type StoreDraft,
} from "./StoreConfigForm";

/// Kept out of the S3 form's shape on purpose: a folder needs one value and
/// a bucket needs six, and a single struct covering both would make "a
/// folder with an access key" expressible.

function kindLabel(kind: StoreConfigView["kind"]): string {
  const labels: Record<StoreConfigView["kind"], string> = {
    s3: t("backup.kind_s3"),
    folder: t("backup.kind_folder"),
    "web-dav": "WebDAV",
    sftp: "SFTP",
    ...CLOUD_NAME,
  };
  return labels[kind];
}

/**
 * The saved connection, shown back without inputs.
 *
 * Read-only on purpose: fields that look editable suggest each one saves on
 * its own, while the backend replaces the connection as a whole. Editing
 * goes through the form, and what is shown here is whatever the backend
 * says it kept.
 */
function StoredSummary({ stored }: { stored: StoreConfigView }) {
  useLocale();
  const rows: [string, string][] = [[t("backup.summary_type"), kindLabel(stored.kind)]];
  if (stored.kind === "s3") {
    rows.push(
      [t("backup.s3_bucket"), stored.prefix ? `${stored.bucket}/${stored.prefix}` : stored.bucket],
      [t("backup.s3_endpoint"), stored.endpoint],
      [t("backup.s3_region"), stored.region],
      [t("backup.summary_access_key"), stored.access_key_id],
    );
  } else if (stored.kind === "folder") {
    rows.push([t("backup.summary_path"), stored.path]);
  } else if (stored.kind === "web-dav") {
    rows.push(
      [t("backup.field_address"), stored.url],
      [t("backup.field_username"), stored.username],
    );
  } else if (isCloudView(stored)) {
    rows.push(
      [t("backup.summary_account"), stored.account],
      [t("backup.field_folder"), stored.folder],
    );
  } else {
    rows.push(
      [t("backup.field_server"), `${stored.username}@${stored.host}:${stored.port}`],
      [t("backup.field_folder"), stored.path || "/"],
      [
        t("backup.summary_sign_in"),
        stored.auth_method === "key" ? t("backup.word_private_key") : t("backup.word_password"),
      ],
    );
    if (stored.host_fingerprint) rows.push([t("backup.summary_server_key"), stored.host_fingerprint]);
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

/** A saved WebDAV connection back in the form, on the kDrive tab when it is one. */
function davFromStored(url: string, username: string): StoreDraft["dav"] {
  const kdrive = parseKdriveUrl(url);
  return {
    preset: kdrive ? "kdrive" : "any",
    url,
    username,
    // Secrets are never sent back; blank means "unchanged".
    password: "",
    kdriveId: kdrive?.id ?? "",
    kdriveFolder: kdrive ? kdrive.folder : KDRIVE_DEFAULT_FOLDER,
  };
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
const IDLE: Status = { kind: "idle" };

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
  useLocale();
  const [connected, setConnected] = useState(false);
  // Lasting: a sync started here goes on when the page is left.
  const [status, setStatus] = useLasting<Status>("backup.status", IDLE, (s) => s.kind === "busy");
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
          dav: davFromStored(stored.url, stored.username),
        }));
        setWhere(stored.url);
        return;
      }
      if (isCloudView(stored)) {
        const kind = stored.kind;
        setDraft((prev) => ({
          ...prev,
          kind,
          cloud: {
            ...prev.cloud,
            // No sign-in id: saving without a new sign-in keeps the stored
            // one, for the same folder.
            [kind]: { signIn: null, account: stored.account, freeBytes: null, folder: stored.folder },
          },
        }));
        setWhere(`${CLOUD_NAME[kind]}, ${stored.folder}`);
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
      setStatus({ kind: "error", message: t("backup.still_needed", { fields: missing.join(", ") }) });
      return false;
    }
    return true;
  };

  const handleTest = async () => {
    if (!guard()) return;
    setStatus({ kind: "busy", message: t("backup.status_testing") });
    try {
      await invoke("s3_test_config", { config: payload() });
      setStatus({ kind: "ok", message: t("backup.status_test_ok") });
    } catch (e) {
      setStatus({ kind: "error", message: formatAppError(e) });
    }
  };

  const handleSave = async () => {
    if (!guard()) return;
    setStatus({ kind: "busy", message: t("backup.status_saving") });
    try {
      await invoke("s3_save_config", { config: payload() });
      setConnected(true);
      setExpanded(false);
      setDraft((prev) => ({
        ...prev,
        s3: { ...prev.s3, secretAccessKey: "" },
        dav: { ...prev.dav, password: "" },
      }));
      setStatus({ kind: "ok", message: t("backup.status_saved") });
      void load();
      onActivity();
    } catch (e) {
      setStatus({ kind: "error", message: formatAppError(e) });
    }
  };

  const handleSyncNow = async () => {
    setStatus({ kind: "busy", message: t("backup.syncing") });
    try {
      const report = await invoke<SyncReport>("sync_now");
      // Renames happen when another device claimed a name first. Surfacing
      // them matters more than the counts: a file the user knows by name is
      // now called something else.
      const renamed =
        report.renamed.length > 0
          ? ` ${t("backup.renamed", { names: report.renamed.join(", ") })}`
          : "";
      setStatus(syncOutcome(report, renamed));
      const s = await invoke<{ pending_ops: number }>("sync_status");
      setPending(s.pending_ops);
      onActivity();
    } catch (e) {
      setStatus({ kind: "error", message: formatAppError(e) });
    }
  };

  const handleDisconnect = async () => {
    setStatus({ kind: "busy", message: t("backup.status_disconnecting") });
    try {
      await invoke("s3_disconnect");
      setConnected(false);
      setStored(null);
      setDraft(EMPTY_STORE_DRAFT);
      setWhere("");
      setStatus({ kind: "ok", message: t("backup.status_disconnected") });
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
                <h3>{t("backup.backing_up_to", { where })}</h3>
                {/* File content is asked about separately from records,
                    because it goes separately: records are pushed first and
                    the blobs follow, so a pass can deliver every record and
                    still leave a file behind. "Everything is backed up" is
                    only true when both queues are empty. */}
                <p>{backupHeadline(pending, unsyncedCount, unsyncedBytes, lastSyncAt)}</p>
                {syncError && status.kind === "idle" && (
                  <p className="hint is-error" role="status">
                    {t("backup.last_sync_failed", { error: formatAppError(syncError) })}
                  </p>
                )}
              </>
            ) : (
              <>
                <h3>{t("backup.not_backed_up")}</h3>
                <p>{t("backup.not_backed_up_body")}</p>
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
            <button
              className="btn-primary"
              type="button"
              disabled={working}
              onClick={() => void handleSyncNow()}
            >
              {status.kind === "busy" ? (
                <span className="spinner" aria-hidden />
              ) : (
                <RefreshCw size={15} />
              )}
              {status.kind === "busy" ? t("backup.syncing") : t("backup.sync_now")}
            </button>
            <button type="button" className="btn-secondary" onClick={() => setExpanded(true)}>
              <Pencil size={15} />
              {t("backup.edit")}
            </button>
            {onTestBackup && (
              <button
                type="button"
                className="btn-secondary"
                disabled={working}
                onClick={onTestBackup}
              >
                <SearchCheck size={15} />
                {t("backup.test_backup")}
              </button>
            )}
            <button
              type="button"
              className="btn-danger"
              disabled={working}
              onClick={() => setConfirmingDisconnect(true)}
            >
              <Unplug size={15} />
              {t("backup.disconnect")}
            </button>
          </div>
        )}

        {!expanded && connected && onTestBackup && (
          <p className="hint">
            {lastTestedAt
              ? t("backup.last_tested", { date: formatDay(Math.floor(lastTestedAt / 1000)) })
              : t("backup.never_tested")}
          </p>
        )}
      </div>

      {confirmingDisconnect && (
        <ConfirmDialog
          title={t("backup.disconnect_title")}
          message={t("backup.disconnect_body", { where })}
          confirmLabel={t("backup.disconnect")}
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
            {t("backup.on_this_computer")}
          </h3>
          {absentCount > 0 && (
            <p className="hint">{t("backup.absent", { count: absentCount })}</p>
          )}
          {missingCount > 0 ? (
            <>
              <p>
                {t("backup.missing_here", { count: missingCount, size: formatBytes(missingBytes) })}
              </p>
              <div className="actions">
                {contentFetch ? (
                  <>
                    <button className="btn-primary" type="button" disabled>
                      {t("backup.downloading", {
                        done: contentFetch.done,
                        total: contentFetch.total,
                      })}
                    </button>
                    <button type="button" className="btn-secondary" onClick={onCancelFetchContent}>
                      {t("backup.stop")}
                    </button>
                  </>
                ) : (
                  <button
                    className="btn-primary"
                    type="button"
                    disabled={busy}
                    onClick={onFetchAllContent}
                  >
                    <HardDriveDownload size={15} />
                    {t("backup.download_all")}
                  </button>
                )}
              </div>
              <p className="hint">{t("backup.download_stop_hint")}</p>
            </>
          ) : (
            <p>{t("backup.all_here", { size: formatBytes(localBytes) })}</p>
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
              {t("backup.full_copy")}
              <span className="hint">{t("backup.full_copy_hint")}</span>
            </span>
          </label>
        </div>
      )}

      {(expanded || !connected) && (
        <div className="panel-section">
          <h3>
            <Server size={16} />
            {connected ? t("backup.change_storage") : t("backup.choose_storage")}
          </h3>
          <p>
            {t("backup.choose_intro")}
            {connected && ` ${t("backup.choose_replaces")}`}
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
              <button
                className="btn-primary"
                type="button"
                disabled={working}
                onClick={() => void handleSave()}
              >
                {status.kind === "busy" && <span className="spinner" aria-hidden />}
                {status.kind === "busy" ? t("backup.working") : t("backup.save_connect")}
              </button>
              <button
                type="button"
                className="btn-secondary"
                disabled={working}
                onClick={() => void handleTest()}
              >
                {t("backup.test_connection")}
              </button>
              {connected && (
                <button
                  type="button"
                  className="btn-secondary"
                  disabled={working}
                  onClick={() => {
                    discardSignIns(draft);
                    setStatus({ kind: "idle" });
                    setExpanded(false);
                  }}
                >
                  {t("common.cancel")}
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
          {t("backup.how_title")}
        </h3>
        <ul className="backup-points">
          <li>
            <span className="backup-point-icon" aria-hidden>
              <LockKeyhole size={16} />
            </span>
            <div>
              <strong>{t("backup.how_encrypted_title")}</strong>
              <p>{t("backup.how_encrypted")}</p>
            </div>
          </li>
          <li>
            <span className="backup-point-icon" aria-hidden>
              <Server size={16} />
            </span>
            <div>
              <strong>{t("backup.how_own_title")}</strong>
              <p>{t("backup.how_own")}</p>
            </div>
          </li>
          <li>
            <span className="backup-point-icon" aria-hidden>
              <Laptop size={16} />
            </span>
            <div>
              <strong>{t("backup.how_sync_title")}</strong>
              <p>{t("backup.how_sync")}</p>
            </div>
          </li>
          <li>
            <span className="backup-point-icon" aria-hidden>
              <HardDriveDownload size={16} />
            </span>
            <div>
              <strong>{t("backup.how_back_title")}</strong>
              <p>{tx("backup.how_back", { action: <em>{t("welcome.join")}</em> })}</p>
            </div>
          </li>
        </ul>
        <p className="hint">{t("backup.how_per_silo")}</p>
      </div>
      )}
    </div>
  );
}
