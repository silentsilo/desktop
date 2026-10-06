import { useCallback, useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ScrollText } from "lucide-react";
import { formatAppError } from "../lib/errors";
import type { AuditStatus } from "../lib/types";
import { AuditLogList } from "./AuditLogList";

type Props = {
  busy: boolean;
  /** Told whenever the log changes, so the notice elsewhere follows. */
  onChanged: (status: AuditStatus) => void;
  devices: { id: string; label: string | null; system_name: string | null }[];
  /** Shown instead of the log when this silo keeps none: its list of changes. */
  fallback: ReactNode;
};

/** How long an organisation's log keeps its records; `null` keeps them. */
const RETENTION_CHOICES: { days: number | null; label: string }[] = [
  { days: 90, label: "90 days" },
  { days: 365, label: "1 year" },
  { days: 1095, label: "3 years" },
  { days: null, label: "Keep everything" },
];

function retentionValue(days: number | null): string {
  return days === null ? "kept" : String(days);
}

function retentionDays(value: string): number | null {
  return value === "kept" ? null : Number.parseInt(value, 10);
}

/**
 * The silo's activity log: whether it is kept, the switch for a personal
 * silo, what an organisation's silo offers whoever holds its keys, and the
 * log itself. A silo that keeps none shows its list of changes instead.
 */
export function AuditLogPanel({ busy, onChanged, devices, fallback }: Props) {
  const [status, setStatus] = useState<AuditStatus | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [startRetention, setStartRetention] = useState("365");

  useEffect(() => {
    let live = true;
    invoke<AuditStatus>("audit_status")
      .then((s) => live && setStatus(s))
      .catch((e) => live && setError(formatAppError(e)));
    return () => {
      live = false;
    };
  }, []);

  /// Runs a change, takes the status it returns, and asks for a sync so the
  /// copies hear of it. A silo with no copies has nothing to sync, and that
  /// is not an error here.
  const change = useCallback(
    async (command: string, args: Record<string, unknown>) => {
      setSaving(true);
      setError(null);
      setNotice(null);
      try {
        const next = await invoke<AuditStatus>(command, args);
        setStatus(next);
        onChanged(next);
        void invoke("sync_now").catch(() => undefined);
      } catch (e) {
        setError(formatAppError(e));
      } finally {
        setSaving(false);
      }
    },
    [onChanged],
  );

  const expire = useCallback(async () => {
    setSaving(true);
    setError(null);
    setNotice(null);
    try {
      const removed = await invoke<number>("audit_org_expire");
      setNotice(
        removed === 0
          ? "Nothing in the log is past the retention."
          : `Removed ${removed} ${removed === 1 ? "segment" : "segments"} past the retention.`,
      );
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setSaving(false);
    }
  }, []);

  const disabled = busy || saving || status === null;

  let body: ReactNode;
  if (status?.organisation) {
    body = (
      <>
        <p>
          This silo keeps an activity log for its organisation, and it stays on. Every device
          records what is done with the silo; only an organisation key reads the log. Reading it,
          changing how long it is kept and removing old records each ask for one.
        </p>
        <div className="settings-row">
          <label className="settings-row-label" htmlFor="audit-retention">
            Keep records for
          </label>
          <select
            id="audit-retention"
            className="auto-lock-select"
            value={retentionValue(status.retention_days)}
            disabled={disabled}
            onChange={(e) =>
              void change("audit_org_retention", { retentionDays: retentionDays(e.target.value) })
            }
          >
            {RETENTION_CHOICES.map((c) => (
              <option key={retentionValue(c.days)} value={retentionValue(c.days)}>
                {c.label}
              </option>
            ))}
          </select>
        </div>
        {status.retention_days !== null && (
          <div className="actions">
            <button
              type="button"
              className="secondary"
              disabled={disabled}
              onClick={() => void expire()}
            >
              Remove records past the retention
            </button>
          </div>
        )}
        <p className="hint">
          Records are removed only from copies that allow deleting. A copy kept as never-delete
          keeps them.
        </p>
      </>
    );
  } else if (status?.org_controlled) {
    body = (
      <>
        <p>
          This silo is administered by an organisation. Its activity log records what is done with
          the silo on every device, and only the organisation's keys read it. Once started it stays
          on, and everyone using the silo is told so.
        </p>
        <div className="settings-row">
          <label className="settings-row-label" htmlFor="audit-start-retention">
            Keep records for
          </label>
          <select
            id="audit-start-retention"
            className="auto-lock-select"
            value={startRetention}
            disabled={disabled}
            onChange={(e) => setStartRetention(e.target.value)}
          >
            {RETENTION_CHOICES.map((c) => (
              <option key={retentionValue(c.days)} value={retentionValue(c.days)}>
                {c.label}
              </option>
            ))}
          </select>
        </div>
        <div className="actions">
          <button
            type="button"
            disabled={disabled}
            onClick={() =>
              void change("audit_org_start", { retentionDays: retentionDays(startRetention) })
            }
          >
            Start the organisation's activity log
          </button>
        </div>
        <p className="hint">Asks for one of the organisation's security keys.</p>
      </>
    );
  } else {
    body = (
      <>
        <p>
          A record of what is done with this silo: unlocking, showing or copying a secret, opening
          or saving a file outside the silo, and changes to entries, files, keys and the recovery
          code. Each record is encrypted on the device that made it before it is stored with the
          silo's copies. Anyone who can open this silo can read the log.
        </p>
        <label className="s3-checkbox">
          <input
            type="checkbox"
            checked={status?.enabled ?? false}
            disabled={disabled}
            onChange={(e) => void change("audit_set_enabled", { enabled: e.target.checked })}
          />
          <span>
            Keep an activity log for this silo
            <span className="hint">
              This computer records from now on, other devices once they sync. Turning it off stops
              new records; the ones already kept stay.
            </span>
          </span>
        </label>
      </>
    );
  }

  return (
    <>
      <div className="panel-section">
        <h3>
          <ScrollText size={16} />
          Activity log
        </h3>
        {body}
        {status && status.waiting > 0 && (
          <p className="hint">
            {status.waiting === 1
              ? "1 record is on this computer and not yet on every copy."
              : `${status.waiting} records are on this computer and not yet on every copy.`}
          </p>
        )}
        {notice && <p className="hint">{notice}</p>}
        {error && <p className="hint is-error">{error}</p>}
      </div>
      {status?.kept ? (
        // Read again when the log changes, so its own event shows.
        <AuditLogList
          key={`${status.enabled}-${status.organisation}`}
          devices={devices}
          needsKey={status.organisation}
        />
      ) : (
        fallback
      )}
    </>
  );
}
