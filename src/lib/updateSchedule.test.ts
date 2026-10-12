import { describe, expect, it } from "vitest";
import {
  isNewerVersion,
  pendingToShow,
  shouldCheckForUpdate,
  UPDATE_CHECK_INTERVAL_MS,
} from "./updateSchedule";

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

describe("an update an earlier check found", () => {
  it("is shown again without asking, while it is newer than what runs", () => {
    expect(pendingToShow("1.5.0", "1.4.0")).toBe("1.5.0");
    expect(pendingToShow("1.5.0", "1.5.0")).toBeNull();
    expect(pendingToShow(null, "1.4.0")).toBeNull();
  });

  it("is ordered as semver orders it", () => {
    expect(isNewerVersion("1.5.0", "1.5.0-rc.4")).toBe(true);
    expect(isNewerVersion("1.5.0-rc.5", "1.5.0-rc.4")).toBe(true);
    expect(isNewerVersion("1.5.0-rc.10", "1.5.0-rc.9")).toBe(true);
    expect(isNewerVersion("1.5.0-rc.4", "1.5.0")).toBe(false);
    expect(isNewerVersion("1.10.0", "1.9.9")).toBe(true);
    expect(isNewerVersion("1.4.9", "1.5.0")).toBe(false);
    expect(isNewerVersion("not a version", "1.0.0")).toBe(false);
  });
});
