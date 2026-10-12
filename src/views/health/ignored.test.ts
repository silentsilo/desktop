import { describe, expect, it, vi } from "vitest";
import type { PasswordEntry } from "../../lib/types";
import type { HealthFinding } from "./analysis";
import { canIgnore, fingerprint, loadIgnored, saveIgnored, splitIgnored } from "./ignored";

function finding(
  id: string,
  severity: HealthFinding["severity"],
  ids: string[] = [],
): HealthFinding {
  return {
    id,
    severity,
    title: id,
    detail: "",
    entries: ids.map((e) => ({ id: e }) as PasswordEntry),
  };
}

const KEY = "a".repeat(64);

describe("ignored health findings", () => {
  it("never sets aside a critical finding", () => {
    const weak = finding("weak", "high", ["a"]);
    expect(canIgnore(weak)).toBe(false);
    const { active, ignored } = splitIgnored([weak], new Set([fingerprint(weak, KEY)]), KEY);
    expect(active).toEqual([weak]);
    expect(ignored).toEqual([]);
  });

  it("sets aside what was ignored, whatever the entry order", () => {
    const stale = finding("stale", "medium", ["b", "a"]);
    const noTotp = finding("no-totp", "info", ["c"]);
    const { active, ignored } = splitIgnored(
      [stale, noTotp],
      new Set([fingerprint(finding("stale", "medium", ["a", "b"]), KEY)]),
      KEY,
    );
    expect(active).toEqual([noTotp]);
    expect(ignored).toEqual([stale]);
  });

  it("shows a finding again once it names another entry", () => {
    const before = finding("stale", "medium", ["a", "b"]);
    const after = finding("stale", "medium", ["a", "b", "c"]);
    const { active } = splitIgnored([after], new Set([fingerprint(before, KEY)]), KEY);
    expect(active).toEqual([after]);
  });
});

describe("what is kept for an ignored finding", () => {
  it("names no entry, and means nothing under another silo's key", () => {
    const stale = finding("stale", "medium", ["entry-one", "entry-two"]);
    const kept = fingerprint(stale, KEY);
    expect(kept).toMatch(/^[0-9a-f]{64}$/);
    expect(kept).not.toContain("entry-one");
    expect(fingerprint(stale, "b".repeat(64))).not.toBe(kept);
  });

  it("drops what an earlier version stored in clear", () => {
    const store = new Map<string, string>();
    vi.stubGlobal("window", {
      localStorage: {
        getItem: (k: string) => store.get(k) ?? null,
        setItem: (k: string, v: string) => store.set(k, v),
      },
    });
    const silo = "silo-x";
    store.set(
      `silentsilo.health.ignored.${silo}`,
      JSON.stringify(["stale:a,b", fingerprint(finding("stale", "medium", ["a"]), KEY)]),
    );
    expect([...loadIgnored(silo)]).toHaveLength(1);
    saveIgnored(silo, loadIgnored(silo));
    expect(store.get(`silentsilo.health.ignored.${silo}`)).not.toContain("stale:a,b");
    vi.unstubAllGlobals();
  });
});
