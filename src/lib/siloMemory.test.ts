import { beforeEach, describe, expect, it, vi } from "vitest";
import { forgetSiloStorage, isDue, lastDone, markDone } from "./siloMemory";

describe("siloMemory", () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => store.set(k, v),
      removeItem: (k: string) => store.delete(k),
      key: (i: number) => [...store.keys()][i] ?? null,
      get length() {
        return store.size;
      },
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

  it("leaves nothing of a forgotten silo", () => {
    markDone("a", "verified", 1000);
    markDone("a", "kit-printed", 1000);
    markDone("b", "verified", 1000);
    localStorage.setItem("silentsilo.health.ignored.a", "[]");
    forgetSiloStorage("a");
    expect(lastDone("a", "verified")).toBeNull();
    expect(lastDone("a", "kit-printed")).toBeNull();
    expect(localStorage.getItem("silentsilo.health.ignored.a")).toBeNull();
    expect(lastDone("b", "verified")).toBe(1000);
  });
});
