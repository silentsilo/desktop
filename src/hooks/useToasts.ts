import { useCallback, useRef, useState } from "react";
import { formatAppError, isLockedError } from "../lib/errors";
import type { Toast, ToastKind } from "../lib/types";

let toastSeq = 0;

/** Most toasts on screen at once. Older ones go to make room. */
export const MAX_VISIBLE = 5;

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

/** What [`plan`] decided to do with an incoming toast. */
export type ToastPlan =
  | { action: "repeat"; id: string }
  | { action: "add"; list: Toast[]; evicted: Toast[] };

/**
 * Where a new toast goes, given what is on screen.
 *
 * Separated from the hook so it can be tested without a renderer: the rules
 * are the interesting part, and the hook is only timers around them.
 *
 * The same thing said twice is still one thing, so a message identical to
 * one already showing refreshes that instead of stacking. A bulk action
 * failing the same way for every file it touches, which is what locking
 * mid-operation does, would otherwise bury the screen in copies of one
 * sentence.
 */
export function plan(current: Toast[], incoming: Toast): ToastPlan {
  const existing = current.find((t) => t.kind === incoming.kind && t.message === incoming.message);
  if (existing) {
    return { action: "repeat", id: existing.id };
  }
  const keep = current.slice(-(MAX_VISIBLE - 1));
  return {
    action: "add",
    list: [...keep, incoming],
    evicted: current.slice(0, current.length - keep.length),
  };
}

export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const list = useRef<Toast[]>([]);
  const timers = useRef<Map<string, number>>(new Map());

  const commit = useCallback((next: Toast[]) => {
    list.current = next;
    setToasts(next);
  }, []);

  const clearTimer = useCallback((id: string) => {
    const timer = timers.current.get(id);
    if (timer) window.clearTimeout(timer);
    timers.current.delete(id);
  }, []);

  const dismiss = useCallback(
    (id: string) => {
      clearTimer(id);
      commit(list.current.filter((x) => x.id !== id));
    },
    [clearTimer, commit],
  );

  const push = useCallback(
    (kind: ToastKind, message: string, ms?: number) => {
      // Scaled with the message unless the caller chose. A flat 5.5s was
      // right for "Folder created." and wrong for the forty-word warnings
      // about append-only copies, which vanished mid-read.
      const duration = ms ?? toastDuration(message);
      const arm = (id: string) => {
        clearTimer(id);
        if (duration > 0) {
          timers.current.set(
            id,
            window.setTimeout(() => dismiss(id), duration),
          );
        }
      };

      const decided = plan(list.current, { id: `t-${toastSeq + 1}`, kind, message });
      if (decided.action === "repeat") {
        arm(decided.id);
        return decided.id;
      }

      toastSeq++;
      for (const gone of decided.evicted) {
        clearTimer(gone.id);
      }
      commit(decided.list);
      const id = decided.list[decided.list.length - 1]!.id;
      arm(id);
      return id;
    },
    [clearTimer, commit, dismiss],
  );

  /// Returns an id for callers that keep one, and an empty string for the
  /// locked case, which is deliberately not shown.
  const error = useCallback(
    (err: unknown) => (isLockedError(err) ? "" : push("error", formatAppError(err))),
    [push],
  );
  const success = useCallback((message: string) => push("success", message), [push]);
  const info = useCallback((message: string) => push("info", message), [push]);

  return { toasts, dismiss, push, error, success, info };
}
