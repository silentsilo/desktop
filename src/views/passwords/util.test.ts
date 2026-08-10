import { describe, expect, it } from "vitest";
import type { PasswordEntry } from "../../lib/types";
import {
  copyKindFor,
  exportNeedsTouch,
  faviconUrl,
  normalizeUrl,
  notesAreSecret,
  oneClickCopyValue,
} from "./util";

function entry(over: Partial<PasswordEntry> = {}): PasswordEntry {
  return {
    id: "e1",
    service: "Example",
    username: "me@example.com",
    password: "hunter2",
    url: "example.com",
    notes: "",
    category: "General",
    created_at: 0,
    updated_at: 0,
    ...over,
  };
}

describe("normalizeUrl", () => {
  it("assumes https for a bare host", () => {
    expect(normalizeUrl("example.com")).toBe("https://example.com");
    expect(normalizeUrl("example.com/login?a=b")).toBe("https://example.com/login?a=b");
  });

  it("keeps an explicit web scheme", () => {
    expect(normalizeUrl("https://example.com")).toBe("https://example.com");
    expect(normalizeUrl("http://example.com")).toBe("http://example.com");
    expect(normalizeUrl("HTTPS://EXAMPLE.COM")).toBe("HTTPS://EXAMPLE.COM");
  });

  it("refuses schemes that are not websites", () => {
    // Entries arrive from CSV and JSON exported by other apps, so this field
    // is untrusted input. The capability scope blocks these too; the point is
    // that the rule does not live only in configuration.
    expect(normalizeUrl("file:///etc/passwd")).toBeNull();
    expect(normalizeUrl("file://C:/Windows/System32/calc.exe")).toBeNull();
    expect(normalizeUrl("smb://server/share")).toBeNull();
    expect(normalizeUrl("javascript:alert(1)")).toBeNull();
    expect(normalizeUrl("data:text/html,<script>")).toBeNull();
  });

  it("treats an empty field as nothing to open", () => {
    expect(normalizeUrl("")).toBeNull();
    expect(normalizeUrl("   ")).toBeNull();
  });
});

describe("faviconUrl", () => {
  it("uses the host of a web address", () => {
    expect(faviconUrl("example.com")).toBe("https://example.com/favicon.ico");
  });

  it("fetches nothing for a scheme that is not a website", () => {
    expect(faviconUrl("file:///etc/passwd")).toBeNull();
  });

  it("leaves private and loopback hosts alone", () => {
    // Otherwise the favicon fetch becomes a way to probe the user's own LAN.
    expect(faviconUrl("http://localhost:9200")).toBeNull();
    expect(faviconUrl("http://192.168.1.1")).toBeNull();
    expect(faviconUrl("http://127.0.0.1")).toBeNull();
  });

  it("sees through the forms a dotted quad can be written in", () => {
    // The URL parser normalises all of these to 127.0.0.1 before the check
    // runs, which is worth pinning: it is the reason the check can be a
    // simple dotted-quad match.
    expect(faviconUrl("http://2130706433")).toBeNull();
    expect(faviconUrl("http://0x7f000001")).toBeNull();
    expect(faviconUrl("http://127.1")).toBeNull();
  });

  it("leaves private IPv6 alone, brackets and all", () => {
    // A URL's hostname keeps the brackets, so a check written against the
    // bare address matches nothing and every one of these reached the network.
    expect(faviconUrl("http://[::1]:9200")).toBeNull();
    expect(faviconUrl("http://[::]")).toBeNull();
    expect(faviconUrl("http://[fd00::1]")).toBeNull();
    expect(faviconUrl("http://[fe80::1]")).toBeNull();
    expect(faviconUrl("http://[::ffff:127.0.0.1]")).toBeNull();
  });

  it("still fetches for names that merely start like a private range", () => {
    // "fc"/"fd" are the first hextet of unique-local IPv6, not a prefix any
    // hostname should be judged by.
    expect(faviconUrl("fcbarcelona.com")).toBe("https://fcbarcelona.com/favicon.ico");
    expect(faviconUrl("fdn.fr")).toBe("https://fdn.fr/favicon.ico");
    expect(faviconUrl("[2606:4700::1111]")).toBe("https://[2606:4700::1111]/favicon.ico");
  });
});

describe("what the entry's own re-auth flag covers", () => {
  it("leaves an ordinary entry exactly as it was", () => {
    // The rule the user asked for: a key touch is required only where they
    // ticked the box. Everything else behaves as it always did, including
    // the notes field, which is often just context under a login.
    expect(notesAreSecret(entry())).toBe(false);
    expect(notesAreSecret(entry({ require_reauth: false }))).toBe(false);
    expect(exportNeedsTouch([entry(), entry({ id: "e2" })])).toBe(false);
  });

  it("covers the notes of an entry that asked to be protected", () => {
    // A note sitting beside a protected password is usually where the
    // recovery codes went. Gating the password and printing the note in
    // full underneath it protected nothing.
    expect(notesAreSecret(entry({ require_reauth: true }))).toBe(true);
  });

  it("covers a note-type entry, which is nothing but its note", () => {
    expect(notesAreSecret(entry({ type: "note", require_reauth: true }))).toBe(true);
  });

  it("makes an export ask once when any entry in it asked to be protected", () => {
    // The broadest reveal in the app, and the one place the flag did not
    // reach: copying, editing and opening an attachment all asked, and then
    // the export wrote every password into a plaintext file untouched.
    expect(exportNeedsTouch([entry(), entry({ id: "e2", require_reauth: true })])).toBe(true);
    expect(exportNeedsTouch([])).toBe(false);
  });
});

describe("the one-click copy", () => {
  it("treats a note as a secret", () => {
    // It is free text somebody chose to keep in a password manager, and the
    // ordinary clipboard on Windows writes what it holds into Clipboard
    // History on disk and syncs it to their other machines.
    const note = entry({ type: "note", notes: "backup codes: 8842 1190" });
    expect(copyKindFor(note)).toBe("secret");
    expect(oneClickCopyValue(note)).toBe("backup codes: 8842 1190");
  });

  it("treats passwords and card numbers as secrets", () => {
    expect(copyKindFor(entry())).toBe("secret");
    expect(oneClickCopyValue(entry())).toBe("hunter2");

    const card = entry({ type: "card", card_number: "4111 1111 1111 1111" });
    expect(copyKindFor(card)).toBe("secret");
    expect(oneClickCopyValue(card)).toBe("4111111111111111");
  });

  it("uses the ordinary clipboard for the parts meant to be handed out", () => {
    const identity = entry({ type: "identity", id_email: "me@example.com" });
    expect(copyKindFor(identity)).toBe("plain");
    expect(oneClickCopyValue(identity)).toBe("me@example.com");

    const key = entry({ type: "ssh_key", ssh_public_key: "ssh-ed25519 AAAA" });
    expect(copyKindFor(key)).toBe("plain");
    expect(oneClickCopyValue(key)).toBe("ssh-ed25519 AAAA");
  });
});
