import { useCallback, useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ScrollText } from "lucide-react";
import { formatAppError } from "../lib/errors";
import type { AuditStatus } from "../lib/types";
import { AuditLogList } from "./AuditLogList";

type Props = {
  busy: boolean;
  devices: { id: string; label: string | null; system_name: string | null }[];
  /** Shown instead of the log when this silo keeps none: its list of changes. */
  fallback: ReactNode;
};

/**
 * The silo's activity log: whether it is kept, the switch for a personal
 * silo, and the log itself. An organisation's log is shown as on, with no
 * switch. A silo that keeps none shows its list of changes instead.
 */
export function AuditLogPanel({ busy, devices, fallback }: Props) {
  const [status, setStatus] = useState<AuditStatus | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    invoke<AuditStatus>("audit_status")
      .then((s) => live && setStatus(s))
      .catch((e) => live && setError(formatAppError(e)));
    return () => {
      live = false;
    };
  }, []);

  const toggle = useCallback(async (enabled: boolean) => {
    setSaving(true);
    setError(null);
    try {
      setStatus(await invoke<AuditStatus>("audit_set_enabled", { enabled }));
      // The copies hear of it at the next sync; asked for now. A silo with
      // no copies has nothing to sync, and that is not an error here.
      void invoke("sync_now").catch(() => undefined);
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setSaving(false);
    }
  }, []);

  return (
    <>
      <div className="panel-section">
        <h3>
          <ScrollText size={16} />
          Activity log
        </h3>
        {status?.organisation ? (
          <p>
            This silo's activity log is kept by its organisation and stays on. Every device records
            what is done with the silo; only an organisation key reads the log.
          </p>
        ) : (
          <>
            <p>
              A record of what is done with this silo: unlocking, showing or copying a secret,
              opening or saving a file outside the silo, and changes to entries, files, keys and the
              recovery code. Each record is encrypted on the device that made it before it is stored
              with the silo's copies. Anyone who can open this silo can read the log.
            </p>
            <label className="s3-checkbox">
              <input
                type="checkbox"
                checked={status?.enabled ?? false}
                disabled={busy || saving || status === null}
                onChange={(e) => void toggle(e.target.checked)}
              />
              <span>
                Keep an activity log for this silo
                <span className="hint">
                  This computer records from now on, other devices once they sync. Turning it off
                  stops new records; the ones already kept stay.
                </span>
              </span>
            </label>
          </>
        )}
        {status && status.waiting > 0 && (
          <p className="hint">
            {status.waiting === 1
              ? "1 record is on this computer and not yet on every copy."
              : `${status.waiting} records are on this computer and not yet on every copy.`}
          </p>
        )}
        {error && <p className="hint is-error">{error}</p>}
      </div>
      {status?.kept ? (
        // Read again when the switch moves, so its own event shows.
        <AuditLogList key={String(status.enabled)} devices={devices} />
      ) : (
        fallback
      )}
    </>
  );
}
