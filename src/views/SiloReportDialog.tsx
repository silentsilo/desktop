import { useState } from "react";
import { useModal } from "../hooks/useModal";
import { formatBytes } from "../lib/format";
import { type SiloReport, reportTime, siloReportText } from "../lib/siloReport";

type Props = {
  report: SiloReport;
  onClose: () => void;
};

/// What is on this screen is on the lock screen: the backend builds the
/// report to be safe there, and this renders it without adding anything.
export function SiloReportDialog({ report, onClose }: Props) {
  const cardRef = useModal(onClose);
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    await navigator.clipboard.writeText(siloReportText(report));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 2000);
  };

  const keys = report.keys;
  const f = report.formats;

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={cardRef}
        className="modal-card silo-report-card"
        role="dialog"
        aria-modal="true"
        aria-label={`About ${report.name}`}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="modal-title">About {report.name}</h3>
        <p className="hint">
          What this silo looks like on disk, without opening it. Copy it into a message if
          something here needs explaining.
        </p>

        <div className="modal-body info-body">
          <div className="info-row">
            <span className="info-label">App</span>
            <span>SilentSilo {report.app_version}</span>
          </div>
          <div className="info-row">
            <span className="info-label">Folder</span>
            <span className="silo-report-path">{report.path}</span>
          </div>
          <div className="info-row">
            <span className="info-label">Silo id</span>
            <span>{report.silo_id ?? "no marker on disk"}</span>
          </div>
          <div className="info-row">
            <span className="info-label">Formats</span>
            <span>
              silo files {f.silo_files} · marker {f.marker} · index schema {f.index_schema} ·
              sealed payload {f.sealed_payload} · blob {f.blob} · recovery envelope{" "}
              {f.recovery_envelope}
            </span>
          </div>

          <div className="silo-report-files">
            {report.files.map((file) => (
              <div key={file.name} className="info-row">
                <span className="info-label">{file.name}</span>
                <span className={file.present ? undefined : "silo-report-missing"}>
                  {file.present
                    ? `${formatBytes(file.bytes ?? 0)} · ${reportTime(file.modified)}`
                    : "missing"}
                </span>
              </div>
            ))}
          </div>

          <div className="info-row">
            <span className="info-label">Keys</span>
            <span>
              {keys === null
                ? "cannot be read"
                : keys.total === 0
                  ? "none enrolled"
                  : `${keys.total} enrolled · ${keys.platform} sealed to a machine · ${keys.portable} portable${
                      keys.revoked > 0 ? ` · ${keys.revoked} retired` : ""
                    }`}
            </span>
          </div>
          <div className="info-row">
            <span className="info-label">Recovery</span>
            <span>{report.recovery_envelope ? "envelope present" : "no envelope"}</span>
          </div>
          <div className="info-row">
            <span className="info-label">Working copy</span>
            <span>
              {report.working_copy === null
                ? "none, the last session closed cleanly"
                : `left behind · ${formatBytes(report.working_copy.bytes ?? 0)} · ${reportTime(
                    report.working_copy.modified
                  )}`}
            </span>
          </div>
          {report.sync_provider && (
            <div className="info-row">
              <span className="info-label">Folder sync</span>
              <span>{report.sync_provider}</span>
            </div>
          )}
          <div className="info-row">
            <span className="info-label">Disk</span>
            <span>
              {report.disk_free_bytes === null || report.disk_total_bytes === null
                ? "unknown"
                : `${formatBytes(report.disk_free_bytes)} free of ${formatBytes(
                    report.disk_total_bytes
                  )}`}
            </span>
          </div>
        </div>

        <div className="modal-actions">
          <button type="button" className="secondary" onClick={() => void copy()}>
            {copied ? "Copied" : "Copy"}
          </button>
          <button type="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
