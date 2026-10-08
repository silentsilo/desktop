import { useState } from "react";
import { useModal } from "../hooks/useModal";
import { formatBytes } from "../lib/format";
import { type KeyCounts, type SiloReport, reportTime, siloReportText } from "../lib/siloReport";
import { t, useLocale } from "../i18n";

/** A file's time, or "unknown" in the language in use. */
function shownTime(seconds: number | null): string {
  return seconds === null ? t("start.report_unknown") : reportTime(seconds);
}

function shownKeys(keys: KeyCounts | null): string {
  if (keys === null) return t("start.report_cannot_read");
  if (keys.total === 0) return t("start.report_none_enrolled");
  const counts = { total: keys.total, platform: keys.platform, portable: keys.portable };
  return keys.revoked > 0
    ? t("start.report_key_counts_retired", { ...counts, revoked: keys.revoked })
    : t("start.report_key_counts", counts);
}

/** The working copy line as the dialog shows it. The copied text keeps
 *  English, for whoever reads it to help. */
function shownWorkingCopy(r: SiloReport): string {
  const leftOpen = t("start.report_left_open");
  if (r.working_copy === null) return r.session_left_open ? leftOpen : t("start.report_none");
  const state = r.session_left_open ? leftOpen : t("start.report_kept");
  return [state, formatBytes(r.working_copy.bytes ?? 0), shownTime(r.working_copy.modified)].join(
    " · "
  );
}

type Props = {
  report: SiloReport;
  onClose: () => void;
};

/// What is on this screen is on the lock screen: the backend builds the
/// report to be safe there, and this renders it without adding anything.
export function SiloReportDialog({ report, onClose }: Props) {
  useLocale();
  const cardRef = useModal(onClose);
  const [copied, setCopied] = useState(false);

  const copy = async () => {
    await navigator.clipboard.writeText(siloReportText(report));
    setCopied(true);
    window.setTimeout(() => setCopied(false), 2000);
  };

  const f = report.formats;

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={cardRef}
        className="modal-card silo-report-card"
        role="dialog"
        aria-modal="true"
        aria-label={t("start.about_silo", { name: report.name })}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="modal-title">{t("start.about_silo", { name: report.name })}</h3>
        <p className="hint">{t("start.report_intro")}</p>

        <div className="modal-body info-body">
          <div className="info-row">
            <span className="info-label">{t("start.report_app")}</span>
            <span>SilentSilo {report.app_version}</span>
          </div>
          <div className="info-row">
            <span className="info-label">{t("start.report_folder")}</span>
            <span className="silo-report-path">{report.path}</span>
          </div>
          <div className="info-row">
            <span className="info-label">{t("start.report_silo_id")}</span>
            <span>{report.silo_id ?? t("start.report_no_marker")}</span>
          </div>
          <div className="info-row">
            <span className="info-label">{t("start.report_formats")}</span>
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
                    ? `${formatBytes(file.bytes ?? 0)} · ${shownTime(file.modified)}`
                    : t("start.report_missing")}
                </span>
              </div>
            ))}
          </div>

          <div className="info-row">
            <span className="info-label">{t("start.report_keys")}</span>
            <span>{shownKeys(report.keys)}</span>
          </div>
          <div className="info-row">
            <span className="info-label">{t("start.report_recovery")}</span>
            <span>
              {report.recovery_envelope
                ? t("start.report_envelope_present")
                : t("start.report_no_envelope")}
            </span>
          </div>
          <div className="info-row">
            <span className="info-label">{t("start.report_working_copy")}</span>
            <span>{shownWorkingCopy(report)}</span>
          </div>
          {report.sync_provider && (
            <div className="info-row">
              <span className="info-label">{t("start.report_folder_sync")}</span>
              <span>{report.sync_provider}</span>
            </div>
          )}
          <div className="info-row">
            <span className="info-label">{t("start.report_disk")}</span>
            <span>
              {report.disk_free_bytes === null || report.disk_total_bytes === null
                ? t("start.report_unknown")
                : t("start.report_disk_free", {
                    free: formatBytes(report.disk_free_bytes),
                    total: formatBytes(report.disk_total_bytes),
                  })}
            </span>
          </div>
        </div>

        <div className="modal-actions">
          <button type="button" className="btn-secondary" onClick={() => void copy()}>
            {copied ? t("start.copied") : t("common.copy")}
          </button>
          <button className="btn-primary" type="button" onClick={onClose}>
            {t("start.close")}
          </button>
        </div>
      </div>
    </div>
  );
}
