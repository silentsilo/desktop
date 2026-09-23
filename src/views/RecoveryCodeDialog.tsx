import { AlertTriangle, LifeBuoy } from "lucide-react";
import { useModal } from "../hooks/useModal";

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
  const cardRef = useModal(undefined);
  return (
    <div className="modal-overlay">
      <div
        ref={cardRef}
        className="modal-card"
        role="alertdialog"
        aria-modal="true"
        aria-label="Your new recovery code"
      >
        <h3 className="modal-title">
          <LifeBuoy size={16} /> Your new recovery code
        </h3>
        <div className="modal-body recovery-reveal">
          <p>
            {siloName ? <strong>{siloName}</strong> : "This silo"} has a new recovery code. The
            one you wrote down before no longer opens it. Write this one down now: it is shown
            once.
          </p>
          <code className="recovery-code">{code}</code>
          <p className="hint is-error">
            <AlertTriangle size={14} />
            Lose this code and all your keys and the silo cannot be opened again.
          </p>
        </div>
        <div className="modal-actions">
          <button type="button" onClick={onDone}>
            I&apos;ve written it down
          </button>
        </div>
      </div>
    </div>
  );
}
