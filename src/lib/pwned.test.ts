import { describe, expect, it } from "vitest";
import { RANGE_CONCURRENCY, checkPasswords, parseRange, sha1Hex } from "./pwned";
import type { PasswordEntry } from "./types";

function entry(overrides: Partial<PasswordEntry> = {}): PasswordEntry {
  return {
    id: crypto.randomUUID(),
    service: "svc",
    username: "u",
    password: "password",
    url: "",
    notes: "",
    category: "General",
    created_at: 1,
    updated_at: 1,
    ...overrides,
  };
}

// The published SHA-1 of the literal word "password"; the range API keys
// everything off this split.
const PASSWORD_SHA1 = "5BAA61E4C9B93F3F0682250B6CF8331B7EE68FD8";
const PREFIX = PASSWORD_SHA1.slice(0, 5);
const SUFFIX = PASSWORD_SHA1.slice(5);

describe("sha1Hex", () => {
  it("matches the known vector", async () => {
    expect(await sha1Hex("password")).toBe(PASSWORD_SHA1);
  });
});

describe("parseRange", () => {
  it("reads well-formed lines", () => {
    const body = `${SUFFIX}:42\r\n0000000000000000000000000000000000A:7`;
    const map = parseRange(body);
    expect(map.get(SUFFIX)).toBe(42);
    expect(map.get("0000000000000000000000000000000000A")).toBe(7);
  });

  it("drops padding entries with count zero", () => {
    expect(parseRange(`${SUFFIX}:0`).size).toBe(0);
  });

  it("survives whatever the service starts returning instead", () => {
    // Their format is not ours to rely on: html, json, half a line, a
    // negative count, junk counts, empty body. None of it may throw.
    for (const body of [
      "<html>maintenance</html>",
      '{"error": "gone"}',
      "TOO:SHORT",
      `${SUFFIX}:-5`,
      `${SUFFIX}:many`,
      "",
      ":::::",
    ]) {
      expect(() => parseRange(body)).not.toThrow();
      expect(parseRange(body).size).toBe(0);
    }
  });
});

describe("checkPasswords", () => {
  it("reports an exposed password with every entry that shares it", async () => {
    const a = entry({ id: "a" });
    const b = entry({ id: "b" });
    const report = await checkPasswords([a, b], async (prefix) => {
      expect(prefix).toBe(PREFIX);
      return `${SUFFIX}:1234`;
    });
    expect(report.exposures).toEqual([{ entryIds: ["a", "b"], count: 1234 }]);
    expect(report.checked).toBe(1);
    expect(report.unavailable).toBe(0);
  });

  it("sends only the five-character prefix, never more", async () => {
    const seen: string[] = [];
    await checkPasswords([entry()], async (prefix) => {
      seen.push(prefix);
      return "";
    });
    expect(seen).toEqual([PREFIX]);
  });

  it("reads an absent suffix as not breached", async () => {
    const report = await checkPasswords([entry()], async () => "0000000000000000000000000000000000A:9");
    expect(report.exposures).toEqual([]);
  });

  it("marks passwords unjudged when the service fails, and does not throw", async () => {
    const report = await checkPasswords([entry()], async () => {
      throw new Error("api gone");
    });
    expect(report.exposures).toEqual([]);
    expect(report.unavailable).toBe(1);
  });

  it("keeps checking the rest when one range fails", async () => {
    const strong = entry({ id: "s", password: "correct horse battery staple" });
    const weak = entry({ id: "w", password: "password" });
    const report = await checkPasswords([strong, weak], async (prefix) => {
      if (prefix === PREFIX) return `${SUFFIX}:99`;
      throw new Error("half the api is down");
    });
    expect(report.exposures).toEqual([{ entryIds: ["w"], count: 99 }]);
    expect(report.unavailable).toBe(1);
  });

  it("skips entries without a password", async () => {
    const report = await checkPasswords([entry({ password: "" })], async () => "");
    expect(report.checked).toBe(0);
  });

  it("worst exposure first", async () => {
    const a = entry({ id: "a", password: "password" });
    const b = entry({ id: "b", password: "123456" });
    const sha6 = await sha1Hex("123456");
    const report = await checkPasswords([a, b], async (prefix) => {
      if (prefix === PREFIX) return `${SUFFIX}:10`;
      return `${sha6.slice(5)}:50000`;
    });
    expect(report.exposures.map((e) => e.count)).toEqual([50000, 10]);
  });
});

describe("how hard the service is asked", () => {
  it("keeps only a few requests in flight at once", async () => {
    // Every distinct password has its own prefix, so a real silo means
    // hundreds of them. Sent together they arrive as a burst the service
    // answers with refusals, and the report then reads as "could not
    // check" rather than as "asked too hard".
    const entries = Array.from({ length: 40 }, (_, i) =>
      entry({ id: `e${i}`, password: `unique-password-${i}` }),
    );

    let inFlight = 0;
    let peak = 0;
    const report = await checkPasswords(entries, async () => {
      inFlight++;
      peak = Math.max(peak, inFlight);
      await Promise.resolve();
      inFlight--;
      return "";
    });

    expect(report.checked).toBe(40);
    expect(peak).toBeLessThanOrEqual(RANGE_CONCURRENCY);
    expect(peak).toBeGreaterThan(1);
  });

  it("asks once per prefix however many entries share a password", async () => {
    const shared = [entry({ id: "a" }), entry({ id: "b" }), entry({ id: "c" })];
    const asked: string[] = [];
    await checkPasswords(shared, async (prefix) => {
      asked.push(prefix);
      return "";
    });
    expect(asked).toHaveLength(1);
  });
});
