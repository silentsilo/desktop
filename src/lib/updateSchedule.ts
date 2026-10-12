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

/**
 * An update an earlier check found is shown from what that check left
 * (`pendingToShow`), not asked for again inside the day: a restart within
 * it sends nothing.
 */
export function shouldCheckForUpdate(lastCheckAt: number | null, now: number): boolean {
  if (lastCheckAt === null || !Number.isFinite(lastCheckAt)) return true;
  // A timestamp in the future means the clock moved backwards since the
  // last check. Treating it as valid would silence checks for however far
  // the clock had drifted, so it counts as stale instead.
  if (lastCheckAt > now) return true;
  return now - lastCheckAt >= UPDATE_CHECK_INTERVAL_MS;
}

/** A version's parts: numbers, then the prerelease identifiers, if any. */
function parseVersion(version: string): { core: number[]; pre: string[] } | null {
  const match = version
    .trim()
    .replace(/^v/, "")
    .match(/^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+.*)?$/);
  if (!match) return null;
  return {
    core: [Number(match[1]), Number(match[2]), Number(match[3])],
    pre: match[4] ? match[4].split(".") : [],
  };
}

/** Whether `candidate` is newer than `current`, by semver: a prerelease
 * ranks below the release it leads to. False when either does not read. */
export function isNewerVersion(candidate: string, current: string): boolean {
  const a = parseVersion(candidate);
  const b = parseVersion(current);
  if (!a || !b) return false;
  for (let i = 0; i < 3; i++) {
    if (a.core[i] !== b.core[i]) return a.core[i]! > b.core[i]!;
  }
  if (a.pre.length === 0 || b.pre.length === 0) return a.pre.length === 0 && b.pre.length > 0;
  for (let i = 0; i < Math.max(a.pre.length, b.pre.length); i++) {
    const x = a.pre[i];
    const y = b.pre[i];
    if (x === undefined) return false;
    if (y === undefined) return true;
    if (x === y) continue;
    const xn = /^\d+$/.test(x);
    const yn = /^\d+$/.test(y);
    if (xn && yn) return Number(x) > Number(y);
    if (xn !== yn) return !xn;
    return x > y;
  }
  return false;
}

/** The update an earlier check found, when it is still newer than what
 * runs: shown at a restart without asking the server again. */
export function pendingToShow(pending: string | null, current: string): string | null {
  return pending !== null && isNewerVersion(pending, current) ? pending : null;
}
