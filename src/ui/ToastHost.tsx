import type { Toast } from "../lib/types";

type Props = {
  toasts: Toast[];
  onDismiss: (id: string) => void;
};

export function ToastHost({ toasts, onDismiss }: Props) {
  if (toasts.length === 0) return null;

  return (
    <div className="toast-host" aria-live="polite">
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
