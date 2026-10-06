import { describe, expect, it } from "vitest";
import { alreadySaved, existingEntry, savedEntry } from "./browserSave";
import { withHistory } from "./entryHistory";
import type { BrowserSavePrompt, PasswordEntry } from "./types";

function prompt(over: Partial<BrowserSavePrompt> = {}): BrowserSavePrompt {
  return {
    request_id: "save-1",
    site: "github.com",
    url: "https://github.com",
    label: "github.com",
    username: "alex@example.com",
    password: "new-one",
    existing: null,
    ...over,
  };
}

function entry(over: Partial<PasswordEntry> = {}): PasswordEntry {
  return {
    id: "e1",
    service: "GitHub",
    username: "alex@example.com",
    password: "old-one",
    url: "https://github.com/login",
    notes: "two-factor on",
    category: "Work",
    created_at: 1000,
    updated_at: 1000,
    favorite: true,
    ...over,
  };
}

describe("saving a login from the browser", () => {
  it("files a new login under the site, with the name the person gave", () => {
    const { entry: saved, updated } = savedEntry(prompt(), undefined, " My GitHub ", "alex", 5000);
    expect(updated).toBe(false);
    expect(saved).toMatchObject({
      service: "My GitHub",
      username: "alex",
      password: "new-one",
      url: "https://github.com",
      category: "General",
      created_at: 5000,
      updated_at: 5000,
    });
    expect(saved.id).toMatch(/^[0-9a-f-]{36}$/);
  });

  it("names a new login after the site when the name is left empty", () => {
    expect(savedEntry(prompt(), undefined, "  ", "alex", 5000).entry.service).toBe("github.com");
  });

  it("updates the password and keeps the rest, the old password in the history", () => {
    const before = entry();
    const { entry: saved, updated } = savedEntry(
      prompt(),
      before,
      "ignored",
      "alex@example.com",
      5000,
    );
    expect(updated).toBe(true);
    expect(saved).toEqual({ ...before, password: "new-one", updated_at: 5000 });
    const stored = withHistory(before, saved, 10);
    expect(stored.history?.[0]).toMatchObject({ password: "old-one" });
  });

  it("finds the login to update only when the window has it", () => {
    const p = prompt({ existing: { id: "e1", label: "GitHub" } });
    expect(existingEntry(p, [entry()])?.id).toBe("e1");
    expect(existingEntry(p, [])).toBeUndefined();
    expect(existingEntry(prompt(), [entry()])).toBeUndefined();
  });

  it("knows a login that is already there as typed", () => {
    expect(alreadySaved(prompt({ password: "old-one" }), entry())).toBe(true);
    expect(alreadySaved(prompt(), entry())).toBe(false);
    expect(alreadySaved(prompt({ password: "old-one", username: "other" }), entry())).toBe(false);
    expect(alreadySaved(prompt(), undefined)).toBe(false);
  });
});
