import { sha256Hex } from "../../lib/sha256";
import type { HealthFinding } from "./analysis";

/**
 * Health findings the person chose to ignore, per silo, on this computer.
 * Only what is not critical can be ignored. A finding is remembered as it
 * was: the kind and the entries it named. One that grows, a fourth stale
 * password where there were three, is a new finding and shows again.
 *
 * Kept as digests under a key the silo gives (`health_ignore_key`), so what
 * sits in this computer's web storage says nothing without the silo: not
 * which entries share a password, not which are weak.
 */

const KEY = (siloId: string) => `silentsilo.health.ignored.${siloId}`;

/** What is stored: written by this version, a digest. */
const STORED = /^[0-9a-f]{64}$/;

/** Which finding this is, its kind and the entries it names. Never stored. */
export function findingKey(finding: HealthFinding): string {
  const ids = finding.entries.map((e) => e.id).sort();
  return `${finding.id}:${ids.join(",")}`;
}

/** What is stored for a finding, under the silo's `key`. */
export function fingerprint(finding: HealthFinding, key: string): string {
  return sha256Hex(`${key}\n${findingKey(finding)}`);
}

export function canIgnore(finding: HealthFinding): boolean {
  return finding.severity !== "high";
}

/** What was stored for the silo. Anything else an earlier version wrote,
 * entry ids in clear, is left out and goes at the next save. */
export function loadIgnored(siloId: string): Set<string> {
  try {
    const raw = window.localStorage.getItem(KEY(siloId));
    const list = raw ? (JSON.parse(raw) as unknown) : [];
    return new Set(
      Array.isArray(list)
        ? list.filter((v): v is string => typeof v === "string" && STORED.test(v))
        : [],
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
  key: string,
): { active: HealthFinding[]; ignored: HealthFinding[] } {
  const active: HealthFinding[] = [];
  const aside: HealthFinding[] = [];
  for (const finding of findings) {
    if (canIgnore(finding) && ignored.has(fingerprint(finding, key))) aside.push(finding);
    else active.push(finding);
  }
  return { active, ignored: aside };
}
