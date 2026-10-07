import { ScrollText } from "lucide-react";
import { ViewHeader } from "../components/ViewHeader";
import type { AuditStatus } from "../lib/types";
import { AuditLogList } from "./AuditLogList";
import { t, useLocale } from "../i18n";

type Props = {
  status: AuditStatus | null;
  devices: { id: string; label: string | null; system_name: string | null }[];
  onOpenSettings: () => void;
};

/** What was done with this silo, from every device: its activity log. */
export function ActivityView({ status, devices, onOpenSettings }: Props) {
  useLocale();
  return (
    <div className="activity-view">
      <ViewHeader
        icon={ScrollText}
        title={t("nav.activity")}
        subtitle={status?.organisation ? t("dlg.activity_org_subtitle") : undefined}
      />
      <div className="activity-pane">
        {status === null ? (
          <p className="hint">{t("dlg.activity_reading")}</p>
        ) : status.kept ? (
          // Read again when the log changes, so its own event shows.
          <>
            {!status.enabled && (
              <p className="hint">{t("dlg.activity_off_kept")}</p>
            )}
            <AuditLogList
            key={`${status.enabled}-${status.organisation}`}
            devices={devices}
            needsKey={status.organisation}
            />
          </>
        ) : status.enabled ? (
          // On, with the key still on its way from another device.
          <p className="hint">{t("dlg.activity_waiting")}</p>
        ) : (
          <div className="panel-section">
            <p>{t("dlg.activity_off")}</p>
            <div className="actions">
              <button type="button" className="secondary" onClick={onOpenSettings}>
                {t("dlg.activity_turn_on")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
