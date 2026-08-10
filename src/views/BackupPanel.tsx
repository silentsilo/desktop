import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CheckCircle2,
  CloudUpload,
  HardDriveDownload,
  Laptop,
  LockKeyhole,
  Pencil,
  RefreshCw,
  Server,
  Unplug,
} from "lucide-react";
import type { StoreConfigView } from "../lib/types";
import { formatAppError } from "../lib/errors";
import { formatBytes } from "../lib/format";
import { detectPreset } from "../lib/s3Presets";
import { syncOutcome, type Status, type SyncReport } from "../lib/syncOutcome";
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
  siloName: string;
  /** Unix ms of the last pass that reached the storage, from the app shell. */
  lastSyncAt: number | null;
  /** Lets the shell's status bar refresh right away instead of on its timer. */
  onActivity: () => void;
  /** Content the backup holds that this computer does not. */
  missingCount: number;
  missingBytes: number;
  /** What the content already here occupies, for the same sentence. */
  localBytes: number;
  /** A download-everything pass in flight, counted in files. */
  contentFetch: { done: number; total: number } | null;
  /** Whether this device is meant to hold every blob, not just the index. */
  fullCopy: boolean;
  onFullCopy: (on: boolean) => void;
  onFetchAllContent: () => void;
  onCancelFetchContent: () => void;
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
  siloName,
  lastSyncAt,
  onActivity,
  missingCount,
  missingBytes,
  localBytes,
  contentFetch,
  fullCopy,
  onFullCopy,
  onFetchAllContent,
  onCancelFetchContent,
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
      try {
        const s = await invoke<{ pending_ops: number }>("sync_status");
        setPending(s.pending_ops);
      } catch {
        setPending(0);
      }
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
    setStatus({ kind: "busy", message: "Writing a test object…" });
    try {
      await invoke("s3_test_config", { config: payload() });
      setStatus({ kind: "ok", message: "Connected. The storage is writable." });
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
      setStatus({ kind: "ok", message: "Storage connected." });
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
      setStatus({ kind: "ok", message: "Disconnected. Your files in the storage were left alone." });
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
                <p>
                  {pending > 0
                    ? `${pending} change${pending === 1 ? "" : "s"} waiting to be sent.`
                    : lastSyncAt
                      ? "Everything is backed up."
                      : "Connected. The first pass runs in the background."}
                </p>
              </>
            ) : (
              <>
                <h3>{siloName} is on this computer only</h3>
                <p>
                  If this machine fails, the silo goes with it. Connect storage you control and an
                  encrypted copy lives there too.
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
      </div>

      {confirmingDisconnect && (
        <ConfirmDialog
          title="Disconnect this backup?"
          message={`This silo stops backing up to ${where}. Nothing there is deleted, but from now on this silo lives on this computer alone until storage is connected again.`}
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

      {/* Copies and verification live on their own Settings pages now: each
          one is a question of its own, and stacking all three here made one
          very long scroll with the answers buried in it. */}

      {/* The other direction. Everything above is about content leaving this
          machine; this is about getting it back, which is the question
          someone asks on the day they replace a computer. */}
      {!expanded && connected && (
        <div className="panel-section">
          <h3>
            <HardDriveDownload size={16} />
            On this computer
          </h3>
          {missingCount > 0 ? (
            <>
              <p>
                {missingCount === 1
                  ? "1 file is in the backup but not here"
                  : `${missingCount} files are in the backup but not here`}{" "}
                ({formatBytes(missingBytes)}). Syncing moves the file list, not the contents, so a
                device that just joined or recovered starts with the names and fetches each file
                when you open it.
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
              the backup.
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
                Without this, a device holds the file list and fetches contents when you open
                them, so it is an index rather than a copy: counting it as one of your three
                copies would be wrong. With it, missing files are fetched in the background and
                nothing is evicted to save space.
              </span>
            </span>
          </label>
        </div>
      )}

      {(expanded || !connected) && (
        <div className="panel-section">
          <h3>
            <Server size={16} />
            {connected ? "Change where the backup lives" : "Where should the backup live?"}
          </h3>
          <p>
            Any S3-compatible bucket, a folder or network share, WebDAV, or SFTP.
            {connected && " A silo has one backup connection: saving replaces the current one."}
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
                {status.kind === "busy" ? "Working…" : "Save & connect"}
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
                Files, names and passwords are sealed on this computer first. The storage only
                ever sees ciphertext.
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
                Point a second computer at the same storage and the silo appears there, kept up
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
                On a new computer, choose <em>From backup storage</em> and unlock with your
                security key or recovery code. The storage holds the data; the key opens it.
              </p>
            </div>
          </li>
        </ul>
        <p className="hint">Each silo connects to its own storage, so they can live in different places.</p>
      </div>
      )}
    </div>
  );
}
