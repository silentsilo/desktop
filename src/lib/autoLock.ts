import type { SiloIdleStatus } from "./types";

/**
 * Whether a silo has sat unused long enough to lock itself.
 *
 * Its own setting wins over the app-wide default, and `null` means "follow
 * the default" rather than "never" — the two are different answers and
 * conflating them would leave a silo unlocked indefinitely on a setting the
 * user meant as a deferral.
 *
 * Zero minutes, from either source, is the one way to say never.
 */
export function shouldLock(silo: SiloIdleStatus, defaultMinutes: number): boolean {
  const limit = silo.auto_lock_minutes ?? defaultMinutes;
  if (limit <= 0) return false;
  return silo.idle_seconds >= limit * 60;
}
