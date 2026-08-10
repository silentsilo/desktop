/**
 * When the automatic update check is allowed to run.
 *
 * The policy is one successful check per 24 hours, enforced here rather
 * than by "check on launch": a vault app is typically left running for
 * days, so a launch-only check would never fire again. The caller polls
 * this hourly and the timestamp gates the actual request.
 *
 * The privacy contract this cap supports: the app contacts the release
 * endpoint at most once a day, and the request carries the app version and
 * platform, nothing that identifies a person or a machine.
 */

export const UPDATE_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

/** How often the gate is re-evaluated while the app is running. */
export const UPDATE_POLL_INTERVAL_MS = 60 * 60 * 1000;

export function shouldCheckForUpdate(lastCheckAt: number | null, now: number): boolean {
  if (lastCheckAt === null || !Number.isFinite(lastCheckAt)) return true;
  // A timestamp in the future means the clock moved backwards since the
  // last check. Treating it as valid would silence checks for however far
  // the clock had drifted, so it counts as stale instead.
  if (lastCheckAt > now) return true;
  return now - lastCheckAt >= UPDATE_CHECK_INTERVAL_MS;
}
