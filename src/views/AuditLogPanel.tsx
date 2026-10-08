import { useCallback, useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ScrollText } from "lucide-react";
import { formatAppError } from "../lib/errors";
import type { AuditStatus } from "../lib/types";
import { t, useLocale, type Key } from "../i18n";
import { SettingList, SettingRow, Toggle } from "../components/Setting";

type Props = {
  busy: boolean;
  /** Told whenever the log changes, so the notice elsewhere follows. */
  onChanged: (status: AuditStatus) => void;
};

/** How long an organisation's log keeps its records; `null` keeps them. */
const RETENTION_CHOICES: { days: number | null; label: Key }[] = [
  { days: 90, label: "set.ret_90_days" },
  { days: 365, label: "set.ret_1_year" },
  { days: 1095, label: "set.ret_3_years" },
  { days: null, label: "set.ret_forever" },
];

function retentionValue(days: number | null): string {
  return days === null ? "kept" : String(days);
}

function retentionDays(value: string): number | null {
  return value === "kept" ? null : Number.parseInt(value, 10);
}

/**
 * Settings for the silo's activity: the switch for a personal silo, and
 * what an organisation's silo offers whoever holds its keys. The log itself
 * is its own page, Activity.
 */
export function AuditLogPanel({ busy, onChanged }: Props) {
  useLocale();
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
      setNotice(removed === 0 ? t("set.audit_expire_none") : t("set.audit_expired"));
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
        <p>{t("set.audit_org_intro")}</p>
        <SettingList>
          <SettingRow label={t("set.audit_keep_for")} htmlFor="audit-retention">
          <select
            id="audit-retention"
            value={retentionValue(status.retention_days)}
            disabled={disabled}
            onChange={(e) =>
              void change("audit_org_retention", {
                retentionDays: retentionDays(e.target.value),
              })
            }
          >
            {RETENTION_CHOICES.map((c) => (
              <option key={retentionValue(c.days)} value={retentionValue(c.days)}>
                {t(c.label)}
              </option>
            ))}
          </select>
          </SettingRow>
        </SettingList>
        {status.retention_days !== null && (
          <div className="actions">
            <button
              type="button"
              className="btn-secondary"
              disabled={disabled}
              onClick={() => void expire()}
            >
              {t("set.audit_expire")}
            </button>
          </div>
        )}
        <p className="hint">{t("set.audit_archive")}</p>
      </>
    );
  } else if (status?.org_controlled) {
    body = (
      <>
        <p>{t("set.audit_org_start_intro")}</p>
        <SettingList>
          <SettingRow label={t("set.audit_keep_for")} htmlFor="audit-start-retention">
          <select
            id="audit-start-retention"
            value={startRetention}
            disabled={disabled}
            onChange={(e) => setStartRetention(e.target.value)}
          >
            {RETENTION_CHOICES.map((c) => (
              <option key={retentionValue(c.days)} value={retentionValue(c.days)}>
                {t(c.label)}
              </option>
            ))}
          </select>
          </SettingRow>
        </SettingList>
        <div className="actions">
          <button
            className="btn-primary"
            type="button"
            disabled={disabled}
            onClick={() =>
              void change("audit_org_start", {
                retentionDays: retentionDays(startRetention),
              })
            }
          >
            {t("set.audit_start")}
          </button>
        </div>
        <p className="hint">{t("set.audit_org_key")}</p>
      </>
    );
  } else {
    body = (
      <>
        <SettingList>
          <SettingRow
            label={t("set.audit_record")}
            htmlFor="audit-enabled"
            hint={t("set.audit_record_hint")}
          >
            <Toggle
              id="audit-enabled"
              checked={status?.enabled ?? false}
              disabled={disabled}
              onChange={(on) => void change("audit_set_enabled", { enabled: on })}
            />
          </SettingRow>
        </SettingList>
      </>
    );
  }

  return (
    <>
      <div className="panel-section">
        <h3>
          <ScrollText size={16} />
          {t("nav.activity")}
        </h3>
        {body}
        {status && status.waiting > 0 && (
          <p className="hint">{t("set.audit_waiting", { count: status.waiting })}</p>
        )}
        {notice && <p className="hint">{notice}</p>}
        {error && <p className="hint is-error">{error}</p>}
      </div>
    </>
  );
}
