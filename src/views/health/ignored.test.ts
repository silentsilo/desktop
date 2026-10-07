import { describe, expect, it } from "vitest";
import type { PasswordEntry } from "../../lib/types";
import type { HealthFinding } from "./analysis";
import { canIgnore, fingerprint, splitIgnored } from "./ignored";

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

describe("ignored health findings", () => {
  it("never sets aside a critical finding", () => {
    const weak = finding("weak", "high", ["a"]);
    expect(canIgnore(weak)).toBe(false);
    const { active, ignored } = splitIgnored(
      [weak],
      new Set([fingerprint(weak)]),
    );
    expect(active).toEqual([weak]);
    expect(ignored).toEqual([]);
  });

  it("sets aside what was ignored, whatever the entry order", () => {
    const stale = finding("stale", "medium", ["b", "a"]);
    const noTotp = finding("no-totp", "info", ["c"]);
    const { active, ignored } = splitIgnored(
      [stale, noTotp],
      new Set([fingerprint(finding("stale", "medium", ["a", "b"]))]),
    );
    expect(active).toEqual([noTotp]);
    expect(ignored).toEqual([stale]);
  });

  it("shows a finding again once it names another entry", () => {
    const before = finding("stale", "medium", ["a", "b"]);
    const after = finding("stale", "medium", ["a", "b", "c"]);
    const { active } = splitIgnored([after], new Set([fingerprint(before)]));
    expect(active).toEqual([after]);
  });
});
