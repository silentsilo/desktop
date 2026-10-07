import type { ReactNode } from "react";
import type { Toast } from "../lib/types";
import { t, useLocale } from "../i18n";

type Props = {
  toasts: Toast[];
  onDismiss: (id: string) => void;
  /** Shown above the toasts in the same column, such as a file opening. */
  children?: ReactNode;
};

export function ToastHost({ toasts, onDismiss, children }: Props) {
  useLocale();
  if (toasts.length === 0 && !children) return null;

  return (
    <div className="toast-host" aria-live="polite">
      {children}
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast toast-${toast.kind}`} role="status">
          <p>{toast.message}</p>
          <button
            type="button"
            className="toast-dismiss"
            onClick={() => onDismiss(toast.id)}
            aria-label={t("app.toast_dismiss")}
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
