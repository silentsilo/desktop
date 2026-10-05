import { describe, expect, it } from "vitest";
import {
  HISTORY_BYTES,
  changedLabels,
  restoredFrom,
  reusesOldPassword,
  withHistory,
  withoutHistory,
} from "./entryHistory";
import { withEdits } from "./passwordEntry";
import type { PasswordEntry } from "./types";

function entry(over: Partial<PasswordEntry> = {}): PasswordEntry {
  return {
    id: "e1",
    service: "Bank",
    username: "ana",
    password: "first",
    url: "https://bank.example",
    notes: "",
    category: "",
    created_at: 1000,
    updated_at: 1000,
    ...over,
  };
}

describe("withHistory", () => {
  it("keeps the previous version when the password changes", () => {
    const before = entry();
    const after = withHistory(before, withEdits(before, { password: "second", updated_at: 2000 }), 10);
    expect(after.password).toBe("second");
    expect(after.history).toHaveLength(1);
    expect(after.history?.[0]).toMatchObject({ password: "first", saved_at: 1000 });
  });

  it("adds nothing for a star, a category or an attachment", () => {
    const before = entry();
    for (const change of [
      { favorite: true },
      { category: "Work" },
      { attachments: [{ blob_id: "b", name: "a.pdf", size_bytes: 1, blob_key: "k" }] },
    ]) {
      const after = withHistory(before, withEdits(before, { ...change, updated_at: 2000 }), 10);
      expect(after.history).toBeUndefined();
    }
  });

  it("takes a field going from absent to empty as no change", () => {
    const before = entry();
    const after = withHistory(before, withEdits(before, { fields: [], totp_secret: "" }), 10);
    expect(after.history).toBeUndefined();
  });

  it("counts a custom field as content", () => {
    const before = entry();
    const after = withHistory(
      before,
      withEdits(before, { fields: [{ name: "PIN", value: "1234", hidden: true }] }),
      10,
    );
    expect(after.history).toHaveLength(1);
  });

  it("never copies attachments, the passkey or the history into a version", () => {
    const before = {
      ...entry({ attachments: [{ blob_id: "b", name: "a", size_bytes: 1, blob_key: "k" }] }),
      passkey: { version: 1 },
      history: [{ saved_at: 500, password: "zero" }],
    } as PasswordEntry;
    const after = withHistory(before, withEdits(before, { password: "second" }), 10);
    const version = after.history?.[0] as Record<string, unknown>;
    expect(version.attachments).toBeUndefined();
    expect(version.passkey).toBeUndefined();
    expect(version.history).toBeUndefined();
    expect(after.history?.[1]).toMatchObject({ password: "zero" });
  });

  it("keeps fields this build does not know about in the version", () => {
    const before = { ...entry(), from_the_future: "x" } as PasswordEntry;
    const after = withHistory(before, withEdits(before, { password: "second" }), 10);
    expect((after.history?.[0] as Record<string, unknown>).from_the_future).toBe("x");
  });

  it("keeps ten by default and drops the oldest", () => {
    let current = entry();
    for (let i = 1; i <= 12; i += 1) {
      current = withHistory(current, withEdits(current, { password: `p${i}`, updated_at: 1000 + i }), 10);
    }
    expect(current.history).toHaveLength(10);
    expect(current.history?.[0].password).toBe("p11");
    expect(current.history?.[9].password).toBe("p2");
  });

  it("keeps as many as fit, and no more than the budget", () => {
    let current = entry({ notes: "n".repeat(10_000) });
    for (let i = 1; i <= 60; i += 1) {
      current = withHistory(current, withEdits(current, { password: `p${i}` }), "fit");
    }
    const size = JSON.stringify(current.history).length;
    expect(size).toBeLessThanOrEqual(HISTORY_BYTES);
    expect(current.history!.length).toBeGreaterThan(10);
    expect(current.history!.length).toBeLessThan(60);
  });

  it("does not bring back a history that was cleared", () => {
    const before = withHistory(entry(), withEdits(entry(), { password: "second" }), 10);
    const cleared = withoutHistory(before);
    expect(withHistory(before, cleared, 10).history).toBeUndefined();
  });

  it("adds nothing to a new entry", () => {
    expect(withHistory(undefined, entry(), 10).history).toBeUndefined();
  });
});

describe("restoredFrom", () => {
  it("brings back what a version said and keeps the current one in history", () => {
    const first = entry({ favorite: true, category: "Money" });
    const second = withHistory(first, withEdits(first, { password: "second", updated_at: 2000 }), 10);
    const restored = restoredFrom(second, second.history![0], 3000);
    const saved = withHistory(second, restored, 10);
    expect(saved.password).toBe("first");
    expect(saved.favorite).toBe(true);
    expect(saved.category).toBe("Money");
    expect(saved.updated_at).toBe(3000);
    expect(saved.history?.map((v) => v.password)).toEqual(["second", "first"]);
  });
});

describe("reusesOldPassword", () => {
  it("finds a password the entry had before", () => {
    const first = entry();
    const second = withHistory(first, withEdits(first, { password: "second" }), 10);
    expect(reusesOldPassword(second)).toBe(false);
    const back = withHistory(second, withEdits(second, { password: "first" }), 10);
    expect(reusesOldPassword(back)).toBe(true);
  });
});

describe("changedLabels", () => {
  it("does not count a version's own date as a change", () => {
    const first = entry();
    const second = withHistory(first, withEdits(first, { password: "second" }), 10);
    expect(changedLabels(second.history![0], second)).toEqual(["Password"]);
  });

  it("names what changed in words", () => {
    const older = entry();
    const newer = withEdits(older, {
      password: "second",
      notes: "new",
      card_number: "4111",
      totp_secret: "JBSWY3DP",
      favorite: true,
    });
    expect(changedLabels(older, newer).sort()).toEqual(
      ["Card", "Notes", "Password", "Two-factor code"].sort(),
    );
  });
});
