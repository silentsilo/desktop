import { describe, expect, it } from "vitest";
import { shouldLock } from "./autoLock";

const silo = (idle_seconds: number, auto_lock_minutes: number | null) => ({
  id: "6d3f6f2e-0c1a-4a5e-9c1b-2b1d5f0a7e11",
  idle_seconds,
  auto_lock_minutes,
});

describe("shouldLock", () => {
  it("locks a silo that has passed its own limit", () => {
    expect(shouldLock(silo(5 * 60, 5), 15)).toBe(true);
  });

  it("leaves a silo alone until its limit is actually reached", () => {
    expect(shouldLock(silo(4 * 60 + 59, 5), 15)).toBe(false);
  });

  it("uses the silo's own limit over the default, in both directions", () => {
    // The point of per-silo timeouts: a strict silo locks while a relaxed
    // default would have held it open, and a relaxed one stays open while a
    // strict default would have locked it.
    expect(shouldLock(silo(6 * 60, 5), 60)).toBe(true);
    expect(shouldLock(silo(20 * 60, 60), 5)).toBe(false);
  });

  it("falls back to the default when the silo has no setting of its own", () => {
    expect(shouldLock(silo(16 * 60, null), 15)).toBe(true);
    expect(shouldLock(silo(14 * 60, null), 15)).toBe(false);
  });

  it("treats zero as never, not as immediately", () => {
    // The dangerous misreading: 0 minutes as a limit of zero would lock a
    // silo the instant it opened, on a setting meant to disable locking.
    expect(shouldLock(silo(60 * 60, 0), 15)).toBe(false);
    expect(shouldLock(silo(60 * 60, null), 0)).toBe(false);
  });
});
