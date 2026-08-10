import { describe, expect, it } from "vitest";
import { analyseHealth, summarise, type SiloHealth } from "./analysis";

/// A disk with plenty of room, so the space finding stays out of the way
/// of tests about passwords. Its own cases set it deliberately.
const HEADROOM = 2 * 1024 * 1024 * 1024;
import type { PasswordEntry } from "../../lib/types";

const NOW = 1_800_000_000_000;
const YEAR = 365 * 24 * 60 * 60 * 1000;

/** A silo with nothing wrong at its own level, so a test only sees the
 * findings its entries produce. */
const HEALTHY_SILO: SiloHealth = {
  backupConfigured: true,
  securityKeyCount: 2,
  recoveryCodeSet: true,
  freeBytes: 500 * 1024 * 1024 * 1024,
  headroomBytes: HEADROOM,
};

function entry(overrides: Partial<PasswordEntry> = {}): PasswordEntry {
  return {
    id: crypto.randomUUID(),
    service: "Example",
    username: "user@example.com",
    password: "8Kq!vRz2mNp#4Wd",
    url: "https://example.com",
    notes: "",
    category: "General",
    created_at: NOW,
    updated_at: NOW,
    totp_secret: "JBSWY3DPEHPK3PXP",
    ...overrides,
  };
}

function ids(findings: ReturnType<typeof analyseHealth>): string[] {
  return findings.map((f) => f.id);
}

describe("analyseHealth", () => {
  it("finds nothing wrong with a healthy silo", () => {
    expect(analyseHealth([entry()], HEALTHY_SILO, NOW)).toEqual([]);
  });

  it("groups the entries sharing a password", () => {
    const shared = "8Kq!vRz2mNp#4Wd";
    const findings = analyseHealth(
      [
        entry({ service: "A", password: shared }),
        entry({ service: "B", password: shared }),
        entry({ service: "C", password: "wR7@tYu9zXc$2Lm" }),
      ],
      HEALTHY_SILO,
      NOW,
    );
    const reused = findings.find((f) => f.id === "reused");
    expect(reused?.groups).toHaveLength(1);
    expect(reused?.groups?.[0]?.map((e) => e.service)).toEqual(["A", "B"]);
    expect(reused?.severity).toBe("high");
  });

  it("does not call a password reused because two entries have none", () => {
    const findings = analyseHealth(
      [
        entry({ type: "note", password: "", url: "", totp_secret: undefined }),
        entry({ type: "note", password: "", url: "", totp_secret: undefined, notes: "other" }),
      ],
      HEALTHY_SILO,
      NOW,
    );
    expect(ids(findings)).not.toContain("reused");
  });

  it("flags a weak password", () => {
    const findings = analyseHealth([entry({ password: "abc" })], HEALTHY_SILO, NOW);
    expect(ids(findings)).toContain("weak");
  });

  it("flags a password untouched for over two years", () => {
    const findings = analyseHealth(
      [entry({ updated_at: NOW - 3 * YEAR })],
      HEALTHY_SILO,
      NOW,
    );
    const stale = findings.find((f) => f.id === "stale");
    expect(stale?.entries).toHaveLength(1);
    expect(stale?.severity).toBe("medium");
  });

  it("leaves a password from last year alone", () => {
    const findings = analyseHealth([entry({ updated_at: NOW - YEAR })], HEALTHY_SILO, NOW);
    expect(ids(findings)).not.toContain("stale");
  });

  it("flags a login with a site but no two-factor code", () => {
    const findings = analyseHealth([entry({ totp_secret: undefined })], HEALTHY_SILO, NOW);
    expect(ids(findings)).toContain("no-totp");
  });

  it("does not ask a card or a note for a two-factor code", () => {
    const findings = analyseHealth(
      [entry({ type: "card", totp_secret: undefined })],
      HEALTHY_SILO,
      NOW,
    );
    expect(ids(findings)).not.toContain("no-totp");
  });

  it("reports entries stored twice", () => {
    const findings = analyseHealth([entry({ service: "Dup" }), entry({ service: "Dup" })], HEALTHY_SILO, NOW);
    const duplicates = findings.find((f) => f.id === "duplicates");
    expect(duplicates?.groups?.[0]).toHaveLength(2);
  });

  it("reports a missing recovery code as the most serious silo finding", () => {
    const findings = analyseHealth([], { ...HEALTHY_SILO, recoveryCodeSet: false }, NOW);
    expect(findings[0]?.id).toBe("no-recovery");
    expect(findings[0]?.fix).toBe("recovery");
  });

  it("reports a single enrolled key and no backup", () => {
    const findings = analyseHealth(
      [],
      { ...HEALTHY_SILO, backupConfigured: false, securityKeyCount: 1, freeBytes: null },
      NOW,
    );
    expect(ids(findings)).toEqual(["single-key", "no-backup"]);
  });

  it("puts the serious findings first", () => {
    const findings = analyseHealth(
      [entry({ password: "abc", totp_secret: undefined })],
      { ...HEALTHY_SILO, backupConfigured: false, freeBytes: null },
      NOW,
    );
    expect(findings.map((f) => f.severity)).toEqual(["high", "medium", "info"]);
  });
});

describe("summarise", () => {
  it("counts findings by severity", () => {
    const findings = analyseHealth(
      [entry({ password: "abc", totp_secret: undefined })],
      { ...HEALTHY_SILO, backupConfigured: false, freeBytes: null },
      NOW,
    );
    expect(summarise(findings)).toEqual({ high: 1, medium: 1, info: 1 });
  });
});
