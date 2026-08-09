import { describe, expect, it } from "vitest";

import { serializeEntry, withEdits } from "./passwordEntry";
import type { PasswordEntry } from "./types";

/**
 * An entry as a newer build wrote it: everything this build knows, plus a
 * field it has never heard of. Cast because the whole point is that the type
 * does not describe it.
 */
function fromTheFuture(): PasswordEntry {
  return {
    id: "b8e1b2b4-0000-4000-8000-000000000001",
    service: "bank",
    username: "alex",
    password: "hunter2",
    url: "https://bank.example",
    notes: "",
    category: "Finance",
    created_at: 1,
    updated_at: 2,
    passkey_credential: { id: "abc", private_key: "def" },
  } as unknown as PasswordEntry;
}

describe("withEdits", () => {
  it("keeps fields this build does not know about", () => {
    // The failure this prevents: editing a password on an older device
    // deletes what a newer one added, silently and unrecoverably.
    const edited = withEdits(fromTheFuture(), { password: "changed" });

    expect(edited.password).toBe("changed");
    expect((edited as Record<string, unknown>).passkey_credential).toEqual({
      id: "abc",
      private_key: "def",
    });
  });

  it("does not mutate the entry it was given", () => {
    const original = fromTheFuture();
    withEdits(original, { service: "other" });
    expect(original.service).toBe("bank");
  });

  it("survives the full load, edit and save round trip", () => {
    const stored = serializeEntry(fromTheFuture());
    const loaded = JSON.parse(stored) as PasswordEntry;
    const saved = serializeEntry(withEdits(loaded, { updated_at: 99 }));

    const reloaded = JSON.parse(saved) as Record<string, unknown>;
    expect(reloaded.updated_at).toBe(99);
    expect(reloaded.passkey_credential).toEqual({ id: "abc", private_key: "def" });
  });
});
