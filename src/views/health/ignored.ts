import type { HealthFinding } from "./analysis";

/**
 * Health findings the person chose to ignore, per silo, on this computer.
 * Only what is not critical can be ignored. A finding is remembered as it
 * was: the kind and the entries it named. One that grows, a fourth stale
 * password where there were three, is a new finding and shows again.
 */

const KEY = (siloId: string) => `silentsilo.health.ignored.${siloId}`;

export function fingerprint(finding: HealthFinding): string {
  const ids = finding.entries.map((e) => e.id).sort();
  return `${finding.id}:${ids.join(",")}`;
}

export function canIgnore(finding: HealthFinding): boolean {
  return finding.severity !== "high";
}

export function loadIgnored(siloId: string): Set<string> {
  try {
    const raw = window.localStorage.getItem(KEY(siloId));
    const list = raw ? (JSON.parse(raw) as unknown) : [];
    return new Set(
      Array.isArray(list) ? list.filter((v) => typeof v === "string") : [],
    );
  } catch {
    return new Set();
  }
}

export function saveIgnored(siloId: string, ignored: Set<string>): void {
  try {
    window.localStorage.setItem(KEY(siloId), JSON.stringify([...ignored]));
  } catch {
    // Private storage off: the choice lasts until the window closes.
  }
}

/** The findings to act on, and those set aside. A critical one is never
 * set aside, whatever was stored. */
export function splitIgnored(
  findings: HealthFinding[],
  ignored: Set<string>,
): { active: HealthFinding[]; ignored: HealthFinding[] } {
  const active: HealthFinding[] = [];
  const aside: HealthFinding[] = [];
  for (const finding of findings) {
    if (canIgnore(finding) && ignored.has(fingerprint(finding)))
      aside.push(finding);
    else active.push(finding);
  }
  return { active, ignored: aside };
}
