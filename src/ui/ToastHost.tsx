import type { ReactNode } from "react";
import type { Toast } from "../lib/types";

type Props = {
  toasts: Toast[];
  onDismiss: (id: string) => void;
  /** Shown above the toasts in the same column, such as a file opening. */
  children?: ReactNode;
};

export function ToastHost({ toasts, onDismiss, children }: Props) {
  if (toasts.length === 0 && !children) return null;

  return (
    <div className="toast-host" aria-live="polite">
      {children}
      {toasts.map((t) => (
        <div key={t.id} className={`toast toast-${t.kind}`} role="status">
          <p>{t.message}</p>
          <button type="button" className="toast-dismiss" onClick={() => onDismiss(t.id)} aria-label="Dismiss">
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
