import { beforeEach, describe, expect, it, vi } from "vitest";
import { isDue, lastDone, markDone } from "./siloMemory";

describe("siloMemory", () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => store.set(k, v),
    });
  });

  it("remembers per silo and per event", () => {
    markDone("a", "verified", 1000);
    expect(lastDone("a", "verified")).toBe(1000);
    expect(lastDone("a", "kit-printed")).toBeNull();
    expect(lastDone("b", "verified")).toBeNull();
  });

  it("is due when never done or older than the limit", () => {
    const day = 24 * 60 * 60 * 1000;
    expect(isDue(null, 90)).toBe(true);
    expect(isDue(0, 90, 91 * day)).toBe(true);
    expect(isDue(0, 90, 89 * day)).toBe(false);
  });
});
