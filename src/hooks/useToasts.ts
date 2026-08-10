import { useCallback, useRef, useState } from "react";
import { formatAppError } from "../lib/errors";
import type { Toast, ToastKind } from "../lib/types";

let toastSeq = 0;

/**
 * How long a toast stays, from how long it takes to read.
 *
 * Roughly 200 words a minute plus a beat to notice the toast at all, floored
 * at the old flat duration so short messages behave as before and capped so
 * a long one cannot squat on the corner of the screen: every toast still has
 * its own dismiss button.
 */
export function toastDuration(message: string): number {
  const words = message.trim().split(/\s+/).length;
  return Math.min(15_000, Math.max(5_500, 1_500 + words * 300));
}

export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const timers = useRef<Map<string, number>>(new Map());

  const dismiss = useCallback((id: string) => {
    const t = timers.current.get(id);
    if (t) window.clearTimeout(t);
    timers.current.delete(id);
    setToasts((prev) => prev.filter((x) => x.id !== id));
  }, []);

  const push = useCallback(
    (kind: ToastKind, message: string, ms?: number) => {
      const id = `t-${++toastSeq}`;
      setToasts((prev) => [...prev.slice(-4), { id, kind, message }]);
      // Scaled with the message unless the caller chose. A flat 5.5s was
      // right for "Folder created." and wrong for the forty-word warnings
      // about append-only copies, which vanished mid-read.
      const duration = ms ?? toastDuration(message);
      if (duration > 0) {
        const handle = window.setTimeout(() => dismiss(id), duration);
        timers.current.set(id, handle);
      }
      return id;
    },
    [dismiss],
  );

  const error = useCallback((err: unknown) => push("error", formatAppError(err)), [push]);
  const success = useCallback((message: string) => push("success", message), [push]);
  const info = useCallback((message: string) => push("info", message), [push]);

  return { toasts, dismiss, push, error, success, info };
}
