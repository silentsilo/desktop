import { useState } from "react";
import { useModal } from "../hooks/useModal";

type Props = {
  /** Names the decision, e.g. "Move to trash?" — not the product. */
  title?: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  busy?: boolean;
  /**
   * A second, narrower decision that belongs to the same act, offered as a
   * checkbox rather than a dialog of its own. Chaining two dialogs made the
   * first one read as the whole question and the second as a surprise, and
   * the user had no way back to the first answer once they had given it.
   *
   * Only for choices that are safe left unticked: it starts unticked and the
   * user has to reach for it.
   */
  option?: { label: string; hint?: string };
  onConfirm: (optionChecked: boolean) => void;
  onCancel: () => void;
};

export function ConfirmDialog({
  title = "Are you sure?",
  message,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  danger = false,
  busy = false,
  option,
  onConfirm,
  onCancel,
}: Props) {
  const cardRef = useModal(busy ? undefined : onCancel);
  const [optionChecked, setOptionChecked] = useState(false);

  return (
    <div className="modal-overlay" onClick={busy ? undefined : onCancel}>
      <div
        ref={cardRef}
        className="modal-card"
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="modal-title">{title}</h3>
        <div className="modal-body">
          <p>{message}</p>
          {option && (
            <label className="confirm-option">
              <input
                type="checkbox"
                checked={optionChecked}
                disabled={busy}
                onChange={(e) => setOptionChecked(e.target.checked)}
              />
              <span>
                {option.label}
                {option.hint && <span className="hint">{option.hint}</span>}
              </span>
            </label>
          )}
        </div>
        <div className="modal-actions">
          <button type="button" className="secondary" disabled={busy} onClick={onCancel}>
            {cancelLabel}
          </button>
          <button
            type="button"
            className={danger ? "danger" : undefined}
            disabled={busy}
            // Not autofocused: the destructive answer should be the one the
            // user reaches for deliberately, not the one Enter lands on.
            onClick={() => onConfirm(optionChecked)}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
