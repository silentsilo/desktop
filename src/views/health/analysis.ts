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
export type HealthFix = "backup" | "keys" | "recovery" | "verify";

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
  /** The last sync pass failed, with its reason when there is one. */
  backupFailing: boolean;
  backupError: string | null;
  /** Unix ms of the last backup test on this computer, or null for never.
   * Left out by an app that cannot test a backup, which skips the finding. */
  lastTestedAt?: number | null;
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

/** How long a backup test counts as recent. */
const TEST_DUE_MS = 90 * 24 * 60 * 60 * 1000;

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
        "If one site leaks it, every account with the same password is exposed. Change these first.",
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
      detail: "Short, or made of one kind of character. Use the generator in the editor to replace them.",
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
        "Not wrong by itself, but an old password is more likely to have leaked somewhere.",
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
        "Where the site offers it, a code here means a stolen password alone does not get anyone in.",
      entries: noTotp,
    });
  }

  if (!silo.recoveryCodeSet) {
    findings.push({
      id: "no-recovery",
      severity: "high",
      title: "This silo has no recovery code",
      detail:
        "If you lose every key, nothing can open this silo again.",
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
          ? "Only one key can open this silo"
          : "No key is enrolled",
      detail:
        "Keep a second key somewhere else, so a lost or broken key does not leave the recovery code as the only way in.",
      entries: [],
      fix: "keys",
    });
  }

  if (!silo.backupConfigured) {
    findings.push({
      id: "no-backup",
      severity: "high",
      title: "Not backed up. This silo is only on this computer.",
      detail:
        "If the drive fails, the silo is lost. Backup keeps an encrypted copy in backup storage you control.",
      entries: [],
      fix: "backup",
    });
  } else if (silo.backupFailing) {
    findings.push({
      id: "backup-failing",
      severity: "high",
      title: "Backup is failing",
      detail: silo.backupError
        ? `The last sync did not reach backup storage: ${silo.backupError}`
        : "The last sync did not reach backup storage, so recent changes are only on this computer.",
      entries: [],
      fix: "backup",
    });
  }

  if (
    silo.backupConfigured &&
    silo.lastTestedAt !== undefined &&
    (silo.lastTestedAt === null || now - silo.lastTestedAt > TEST_DUE_MS)
  ) {
    findings.push({
      id: "backup-untested",
      severity: "info",
      title:
        silo.lastTestedAt === null
          ? "The backup has never been tested from this computer"
          : "The backup has not been tested for three months",
      detail:
        "A test reads the backup and compares it with this silo, so a backup that stopped working is found before you need it.",
      entries: [],
      fix: "verify",
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
        `${formatBytes(silo.freeBytes)} free where this silo lives. Adding or downloading ` +
        "files stops partway once the disk is full.",
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
