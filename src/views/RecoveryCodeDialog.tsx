import { AlertTriangle, LifeBuoy } from "lucide-react";
import { useModal } from "../hooks/useModal";
import { t, tx, useLocale } from "../i18n";

type Props = {
  code: string;
  /** The silo it opens, named because the screen behind may be another one. */
  siloName: string | null;
  onDone: () => void;
};

/**
 * A recovery code minted by a key change, shown over whatever screen is up.
 *
 * A key change locks every silo before it returns, so the Settings page that
 * used to show the new code was gone by the time the code arrived, and the
 * lock that follows cleared it. The old code already stopped working, so this
 * one stays on screen until the user says it is written down: no Escape, no
 * backdrop click.
 */
export function RecoveryCodeDialog({ code, siloName, onDone }: Props) {
  useLocale();
  const cardRef = useModal(undefined);
  return (
    <div className="modal-overlay">
      <div
        ref={cardRef}
        className="modal-card"
        role="alertdialog"
        aria-modal="true"
        aria-label={t("recovery_new.title")}
      >
        <h3 className="modal-title">
          <LifeBuoy size={16} /> {t("recovery_new.title")}
        </h3>
        <div className="modal-body recovery-reveal">
          <p>
            {siloName
              ? tx("recovery_new.body_named", { name: <strong>{siloName}</strong> })
              : t("recovery_new.body")}
          </p>
          <code className="recovery-code">{code}</code>
          <p className="hint is-error">
            <AlertTriangle size={14} />
            {t("recovery_new.warning")}
          </p>
        </div>
        <div className="modal-actions">
          <button className="btn-primary" type="button" onClick={onDone}>
            {t("recovery_new.done")}
          </button>
        </div>
      </div>
    </div>
  );
}
