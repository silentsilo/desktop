import { describe, expect, it } from "vitest";
import { shouldCheckForUpdate, UPDATE_CHECK_INTERVAL_MS } from "./updateSchedule";

const NOW = 1_800_000_000_000;

describe("shouldCheckForUpdate", () => {
  it("checks when there is no previous timestamp", () => {
    expect(shouldCheckForUpdate(null, NOW)).toBe(true);
  });

  it("checks when the last check is NaN from a corrupt store", () => {
    expect(shouldCheckForUpdate(Number.NaN, NOW)).toBe(true);
  });

  it("does not check again within the 24h window", () => {
    expect(shouldCheckForUpdate(NOW - 1, NOW)).toBe(false);
    expect(shouldCheckForUpdate(NOW - UPDATE_CHECK_INTERVAL_MS + 1000, NOW)).toBe(false);
  });

  it("checks once the window has elapsed", () => {
    expect(shouldCheckForUpdate(NOW - UPDATE_CHECK_INTERVAL_MS, NOW)).toBe(true);
    expect(shouldCheckForUpdate(NOW - UPDATE_CHECK_INTERVAL_MS - 1, NOW)).toBe(true);
  });

  it("treats a future timestamp as stale, so a clock rollback cannot silence checks", () => {
    expect(shouldCheckForUpdate(NOW + 60_000, NOW)).toBe(true);
  });
});
