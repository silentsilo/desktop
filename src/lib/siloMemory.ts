/**
 * When something worth repeating was last done for a silo: a backup test,
 * a recovery rehearsal, a printed kit.
 *
 * Kept in this computer's web storage, per silo. It only drives reminders,
 * so losing it costs a reminder that comes too early, never data. Every
 * read and write is guarded: storage can be unavailable or cleared.
 */
export type SiloEvent = "verified" | "restore-tested" | "kit-printed";

function key(siloId: string, event: SiloEvent): string {
  return `silentsilo.silo.${siloId}.${event}`;
}

/** Unix ms of the last time, or null when never recorded here. */
export function lastDone(siloId: string, event: SiloEvent): number | null {
  try {
    const value = Number.parseInt(localStorage.getItem(key(siloId, event)) ?? "", 10);
    return Number.isFinite(value) && value > 0 ? value : null;
  } catch {
    return null;
  }
}

export function markDone(siloId: string, event: SiloEvent, at: number = Date.now()): void {
  try {
    localStorage.setItem(key(siloId, event), String(at));
  } catch {
    // A reminder that comes early is the whole cost.
  }
}

/** Whether a record is missing or older than `days`. */
export function isDue(at: number | null, days: number, now: number = Date.now()): boolean {
  return at === null || now - at > days * 24 * 60 * 60 * 1000;
}
