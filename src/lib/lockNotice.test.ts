import { describe, expect, it } from "vitest";
import { countdown, secondsToLock } from "./lockNotice";

describe("the lock countdown", () => {
  it("reads as minutes and seconds", () => {
    expect(countdown(45)).toBe("0:45");
    expect(countdown(60)).toBe("1:00");
    expect(countdown(4.2)).toBe("0:05");
    expect(countdown(-3)).toBe("0:00");
  });

  it("counts from the silo's own limit, else the default, and never for zero", () => {
    expect(secondsToLock(1740, null, 30)).toBe(60);
    expect(secondsToLock(240, 5, 30)).toBe(60);
    expect(secondsToLock(10, null, 0)).toBeNull();
  });
});
