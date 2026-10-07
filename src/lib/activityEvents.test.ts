import { describe as group, expect, it } from "vitest";
import { byDay, dayLabel, describe, KIND_CODES } from "./activityEvents";
import type { AuditEntry } from "./types";

function entry(c: number, over: Partial<AuditEntry> = {}): AuditEntry {
  return { device: "d1", what: "Something", i: 1, t: 0, c, ...over };
}

function sentence(e: AuditEntry): string {
  const d = describe(e);
  return `${d.before}${d.object}${d.after}`;
}

group("activity events", () => {
  it("reads each kind as a sentence naming its object", () => {
    expect(sentence(entry(1, { x: { key: "YubiKey" } }))).toBe("Unlocked with YubiKey");
    expect(sentence(entry(1, { x: { by: "recovery code" } }))).toBe(
      "Unlocked with the recovery code",
    );
    expect(sentence(entry(11, { l: "Bank", x: { field: "password" } }))).toBe(
      "Copied the password of Bank",
    );
    expect(sentence(entry(13, { l: "GitHub", x: { site: "github.com" } }))).toBe(
      "Filled GitHub in the browser",
    );
    expect(describe(entry(13, { l: "GitHub", x: { site: "github.com" } })).details).toEqual([
      "on github.com",
    ]);
    expect(sentence(entry(15, { l: "Deploy", x: { for: "git" } }))).toBe("Signed with Deploy for Git");
    expect(sentence(entry(34, { x: { what: "trash" } }))).toBe("Emptied the trash");
    expect(sentence(entry(34, { x: { count: 3 } }))).toBe("Deleted 3 items for good");
    expect(sentence(entry(40, { x: { count: 1, format: "kdbx" } }))).toBe("Imported 1 password");
    expect(describe(entry(40, { x: { count: 1, format: "kdbx" } })).details).toEqual(["as KDBX"]);
    expect(sentence(entry(53, { x: { now: "off" } }))).toBe("Turned the recovery code off");
  });

  it("marks what deserves a second look", () => {
    expect(describe(entry(51, { l: "Old key" })).tone).toBe("notice");
    expect(describe(entry(3)).tone).toBe("notice");
    expect(describe(entry(2)).tone).toBe("plain");
  });

  it("keeps core's name for a code this build does not know", () => {
    expect(sentence(entry(999, { what: "Unknown event 999", l: "X" }))).toBe(
      "Unknown event 999: X",
    );
  });

  it("puts every known code in exactly one group", () => {
    const all = Object.values(KIND_CODES).flat();
    expect(new Set(all).size).toBe(all.length);
    expect(all).toContain(15);
  });

  it("groups by day, under Today, Yesterday and dates", () => {
    const now = new Date(2026, 9, 7, 12).getTime();
    expect(dayLabel(new Date(2026, 9, 7, 1).getTime(), now)).toBe("Today");
    expect(dayLabel(new Date(2026, 9, 6, 23).getTime(), now)).toBe("Yesterday");
    const groups = byDay(
      [
        { t: new Date(2026, 9, 7, 9).getTime() },
        { t: new Date(2026, 9, 7, 8).getTime() },
        { t: new Date(2026, 9, 6, 8).getTime() },
        { t: new Date(2026, 9, 1, 8).getTime() },
      ],
      now,
    );
    expect(groups.map((g) => [g.label, g.events.length]).slice(0, 2)).toEqual([
      ["Today", 2],
      ["Yesterday", 1],
    ]);
    expect(groups).toHaveLength(3);
  });
});
