import { describe, expect, it } from "vitest";
import { dropDuplicates, entryFingerprint } from "./passwordImport";
import type { PasswordEntry } from "./types";

function entry(overrides: Partial<PasswordEntry> = {}): PasswordEntry {
  return {
    id: crypto.randomUUID(),
    service: "Example",
    username: "user@example.com",
    password: "hunter2",
    url: "https://example.com",
    notes: "",
    category: "General",
    created_at: 0,
    updated_at: 0,
    ...overrides,
  };
}

describe("entryFingerprint", () => {
  it("ignores the id and the timestamps", () => {
    expect(entryFingerprint(entry({ id: "a", created_at: 1, updated_at: 2 }))).toBe(
      entryFingerprint(entry({ id: "b", created_at: 3, updated_at: 4 })),
    );
  });

  it("ignores the category, which is where the entry was filed", () => {
    expect(entryFingerprint(entry({ category: "Work" }))).toBe(
      entryFingerprint(entry({ category: "Development" })),
    );
  });

  it("treats an absent type as login", () => {
    expect(entryFingerprint(entry({ type: "login" }))).toBe(entryFingerprint(entry()));
  });

  it("treats an empty field as an absent one", () => {
    const withEmptyUrl = entry({ url: "" });
    const withoutUrl = entry({ url: "" });
    delete (withoutUrl as Partial<PasswordEntry>).url;
    expect(entryFingerprint(withEmptyUrl)).toBe(entryFingerprint(withoutUrl));
  });

  it("separates entries differing only in the password", () => {
    expect(entryFingerprint(entry({ password: "a" }))).not.toBe(
      entryFingerprint(entry({ password: "b" })),
    );
  });

  it("separates entries differing only in a TOTP secret", () => {
    expect(entryFingerprint(entry({ totp_secret: "JBSWY3DPEHPK3PXP" }))).not.toBe(
      entryFingerprint(entry()),
    );
  });

  it("separates two kinds carrying the same name", () => {
    expect(entryFingerprint(entry({ type: "note", password: "" }))).not.toBe(
      entryFingerprint(entry({ type: "card", password: "" })),
    );
  });
});

describe("dropDuplicates", () => {
  it("keeps everything when the silo is empty", () => {
    const incoming = [entry({ service: "A" }), entry({ service: "B" })];
    expect(dropDuplicates([], incoming)).toEqual({ fresh: incoming, duplicates: 0 });
  });

  it("drops an entry the silo already holds", () => {
    const stored = entry({ service: "GitHub" });
    // Same content, fresh id: what a second import of one file produces.
    const reimported = entry({ service: "GitHub", id: "other" });
    expect(dropDuplicates([stored], [reimported])).toEqual({ fresh: [], duplicates: 1 });
  });

  it("imports an entry whose password changed since the last export", () => {
    const stored = entry({ password: "old" });
    const updated = entry({ password: "new" });
    expect(dropDuplicates([stored], [updated])).toEqual({ fresh: [updated], duplicates: 0 });
  });

  it("drops a row the file itself repeats", () => {
    const first = entry({ service: "Repeated" });
    const second = entry({ service: "Repeated" });
    expect(dropDuplicates([], [first, second])).toEqual({ fresh: [first], duplicates: 1 });
  });

  it("keeps the file order of what survives", () => {
    const stored = entry({ service: "B" });
    const incoming = [entry({ service: "A" }), entry({ service: "B" }), entry({ service: "C" })];
    const { fresh, duplicates } = dropDuplicates([stored], incoming);
    expect(fresh.map((e) => e.service)).toEqual(["A", "C"]);
    expect(duplicates).toBe(1);
  });
});
