import { useSyncExternalStore } from "react";

/**
 * Whether the system says so a minute before a silo locks itself. On unless
 * turned off; kept on this computer, like the app's other settings.
 */
const KEY = "silentsilo.lockNotice";
const listeners = new Set<() => void>();

function read(): boolean {
  try {
    return localStorage.getItem(KEY) !== "off";
  } catch {
    return true;
  }
}

let current = read();

export function setLockNotice(on: boolean) {
  current = on;
  try {
    localStorage.setItem(KEY, on ? "on" : "off");
  } catch {
    // Kept for this session.
  }
  for (const listener of listeners) listener();
}

export function useLockNotice(): boolean {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => current,
    () => current,
  );
}

/** Seconds as the countdown shows them: "0:45". */
export function countdown(seconds: number): string {
  const s = Math.max(0, Math.ceil(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Seconds until a silo idle for `idleSeconds` locks, or null if it never does. */
export function secondsToLock(
  idleSeconds: number,
  ownMinutes: number | null,
  defaultMinutes: number,
): number | null {
  const limit = ownMinutes ?? defaultMinutes;
  if (limit <= 0) return null;
  return limit * 60 - idleSeconds;
}
