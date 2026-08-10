/**
 * What is wrong with what this silo holds.
 *
 * Everything in this module is computed on this machine from entries
 * already in memory; nothing here touches the network, and the checks that
 * matter most (the same password on twelve sites) need no help anyway. The
 * one check that does go out, against Have I Been Pwned, lives beside these
 * findings in the panel and runs only when its button is pressed.
 */

import { entryFingerprint } from "../../lib/passwordImport";
import type { PasswordEntry } from "../../lib/types";
import { formatBytes } from "../../lib/format";
import { passwordStrength, typeOf } from "../passwords/util";

/** How much it matters. High is "fix this today". */
export type HealthSeverity = "high" | "medium" | "info";

/** Where the fix lives, for findings about the silo rather than an entry. */
export type HealthFix = "backup" | "keys" | "recovery";

export type HealthFinding = {
  /** Stable key: the React key, and what the panel remembers as expanded. */
  id: string;
  severity: HealthSeverity;
  title: string;
  /** What actually goes wrong, in one sentence. */
  detail: string;
  /** The entries this is about. Empty for findings about the silo itself. */
  entries: PasswordEntry[];
  /** Set when the entries only mean something in their groups: the point of
   * a reused password is which entries share it. */
  groups?: PasswordEntry[][];
  fix?: HealthFix;
};

/** The parts of the silo's state that can be wrong on their own. */
export type SiloHealth = {
  backupConfigured: boolean;
  securityKeyCount: number;
  recoveryCodeSet: boolean;
  /** Room left where the silo lives, or null when the disk cannot be asked:
   * an unplugged drive, or a platform this build does not query. Null means
   * no warning, because inventing a reassuring number would be worse and
   * inventing an alarming one would cry wolf. */
  freeBytes: number | null;
  /** What the app insists on leaving free beyond whatever it writes. */
  headroomBytes: number;
};

/** Old enough that the account has probably outlived the password. */
const STALE_MS = 2 * 365 * 24 * 60 * 60 * 1000;

/** `passwordStrength` scores 0 to 4; the bottom two are the ones worth
 * naming, and anything above reads as nagging. */
const WEAK_SCORE = 1;

const SEVERITY_ORDER: Record<HealthSeverity, number> = { high: 0, medium: 1, info: 2 };

function byService(a: PasswordEntry, b: PasswordEntry): number {
  return a.service.localeCompare(b.service);
}

/** Groups entries by a key, keeping only the keys that landed more than one. */
function collide(
  entries: PasswordEntry[],
  key: (entry: PasswordEntry) => string | null,
): PasswordEntry[][] {
  const buckets = new Map<string, PasswordEntry[]>();
  for (const entry of entries) {
    const k = key(entry);
    if (k === null) continue;
    const bucket = buckets.get(k);
    if (bucket) bucket.push(entry);
    else buckets.set(k, [entry]);
  }
  return [...buckets.values()]
    .filter((group) => group.length > 1)
    .map((group) => [...group].sort(byService))
    .sort((a, b) => b.length - a.length);
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

export function analyseHealth(
  entries: PasswordEntry[],
  silo: SiloHealth,
  now: number = Date.now(),
): HealthFinding[] {
  const findings: HealthFinding[] = [];

  const reused = collide(entries, (e) => (e.password ? `pw:${e.password}` : null));
  if (reused.length > 0) {
    const affected = reused.flat();
    findings.push({
      id: "reused",
      severity: "high",
      title: `${plural(reused.length, "password is", "passwords are")} used more than once`,
      detail:
        "One leaked site hands over every account sharing that password. These are the entries to change first.",
      entries: affected,
      groups: reused,
    });
  }

  const duplicates = collide(entries, entryFingerprint);
  if (duplicates.length > 0) {
    findings.push({
      id: "duplicates",
      severity: "info",
      title: `${plural(duplicates.length, "entry is", "entries are")} stored twice`,
      detail:
        "Identical copies, usually left by an import. Deleting the spare loses nothing.",
      entries: duplicates.flat(),
      groups: duplicates,
    });
  }

  const weak = entries
    .filter((e) => e.password && passwordStrength(e.password).score <= WEAK_SCORE)
    .sort(byService);
  if (weak.length > 0) {
    findings.push({
      id: "weak",
      severity: "high",
      title: `${plural(weak.length, "password is", "passwords are")} weak`,
      detail: "Short, or built from one kind of character. The generator replaces these in a click.",
      entries: weak,
    });
  }

  const stale = entries
    .filter((e) => typeOf(e) === "login" && e.password && now - e.updated_at > STALE_MS)
    .sort((a, b) => a.updated_at - b.updated_at);
  if (stale.length > 0) {
    findings.push({
      id: "stale",
      severity: "medium",
      title: `${plural(stale.length, "password has", "passwords have")} not changed in two years`,
      detail:
        "Not wrong by itself, but a password that old has usually sat through a breach somewhere.",
      entries: stale,
    });
  }

  const noTotp = entries
    .filter((e) => typeOf(e) === "login" && e.password && e.url && !e.totp_secret)
    .sort(byService);
  if (noTotp.length > 0) {
    findings.push({
      id: "no-totp",
      severity: "info",
      title: `${plural(noTotp.length, "login has", "logins have")} no two-factor code`,
      detail:
        "Where the site offers it, a code here means a stolen password alone is not enough to get in.",
      entries: noTotp,
    });
  }

  if (!silo.recoveryCodeSet) {
    findings.push({
      id: "no-recovery",
      severity: "high",
      title: "This silo has no recovery code",
      detail:
        "Lose the security key and there is no way back in. Nothing anywhere can reconstruct the contents.",
      entries: [],
      fix: "recovery",
    });
  }

  if (silo.securityKeyCount <= 1) {
    findings.push({
      id: "single-key",
      severity: "medium",
      title:
        silo.securityKeyCount === 1
          ? "Only one security key can open this silo"
          : "No security key is enrolled",
      detail:
        "A second key kept somewhere else turns a lost or broken key from a recovery-code emergency into an inconvenience.",
      entries: [],
      fix: "keys",
    });
  }

  if (!silo.backupConfigured) {
    findings.push({
      id: "no-backup",
      severity: "medium",
      title: "This silo exists on this disk only",
      detail: "A failed drive takes it with it. Backup copies encrypted objects to storage you control.",
      entries: [],
      fix: "backup",
    });
  }

  // A silo is an encrypted copy, so everything entering it is written twice,
  // and fetching what another device stored writes here too. A full disk
  // stops both partway through. Worth saying before someone drops a folder
  // on the window rather than after.
  if (silo.freeBytes !== null && silo.freeBytes < silo.headroomBytes) {
    const critical = silo.freeBytes < silo.headroomBytes / 4;
    findings.push({
      id: "low-disk-space",
      severity: critical ? "high" : "medium",
      title: critical ? "This disk is nearly full" : "Not much room left on this disk",
      detail:
        `${formatBytes(silo.freeBytes)} free where this silo lives. Adding files writes an ` +
        "encrypted second copy here, and so does fetching what another device stored, so both " +
        "stop partway through once the disk fills.",
      entries: [],
    });
  }

  return findings.sort((a, b) => {
    const bySeverity = SEVERITY_ORDER[a.severity] - SEVERITY_ORDER[b.severity];
    return bySeverity !== 0 ? bySeverity : b.entries.length - a.entries.length;
  });
}

export type HealthSummary = { high: number; medium: number; info: number };

export function summarise(findings: HealthFinding[]): HealthSummary {
  return {
    high: findings.filter((f) => f.severity === "high").length,
    medium: findings.filter((f) => f.severity === "medium").length,
    info: findings.filter((f) => f.severity === "info").length,
  };
}
