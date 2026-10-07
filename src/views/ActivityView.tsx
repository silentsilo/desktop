import { ScrollText } from "lucide-react";
import { ViewHeader } from "../components/ViewHeader";
import type { AuditStatus } from "../lib/types";
import { AuditLogList } from "./AuditLogList";

type Props = {
  status: AuditStatus | null;
  devices: { id: string; label: string | null; system_name: string | null }[];
  onOpenSettings: () => void;
};

/** What was done with this silo, from every device: its activity log. */
export function ActivityView({ status, devices, onOpenSettings }: Props) {
  return (
    <div className="activity-view">
      <ViewHeader
        icon={ScrollText}
        title="Activity"
        subtitle={status?.organisation ? "Kept by your organisation" : undefined}
      />
      <div className="activity-pane">
        {status === null ? (
          <p className="hint">Reading…</p>
        ) : status.kept ? (
          // Read again when the log changes, so its own event shows.
          <>
            {!status.enabled && (
              <p className="hint">Activity is off. What was recorded before stays here.</p>
            )}
            <AuditLogList
            key={`${status.enabled}-${status.organisation}`}
            devices={devices}
            needsKey={status.organisation}
            />
          </>
        ) : (
          <div className="panel-section">
            <p>Activity is off for this silo.</p>
            <div className="actions">
              <button type="button" className="secondary" onClick={onOpenSettings}>
                Turn it on in Settings
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
