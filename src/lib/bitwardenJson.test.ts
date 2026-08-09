import { describe, expect, it } from "vitest";
import { bitwardenJsonToEntries, JsonImportError } from "./bitwardenJson";

const sample = JSON.stringify({
  encrypted: false,
  folders: [{ id: "f1", name: "Work" }],
  items: [
    {
      type: 1,
      name: "GitHub",
      folderId: "f1",
      login: {
        username: "alex",
        password: "hunter2",
        totp: "JBSWY3DPEHPK3PXP",
        uris: [{ uri: "https://github.com" }],
      },
    },
    { type: 2, name: "Just a note", notes: "text" },
    { type: 99, name: "From the future" },
    {
      type: 3,
      name: "Visa",
      card: {
        cardholderName: "Alex",
        brand: "Visa",
        number: "4111111111111111",
        expMonth: "9",
        expYear: "2027",
        code: "123",
      },
    },
    {
      type: 4,
      name: "Me",
      identity: {
        firstName: "Alex",
        lastName: "Stanciu",
        email: "a@example.com",
        ssn: "123-45-6789",
      },
    },
    {
      type: 5,
      name: "Server key",
      sshKey: {
        privateKey: "-----BEGIN OPENSSH PRIVATE KEY-----",
        publicKey: "ssh-ed25519 AAAA",
        keyFingerprint: "SHA256:abc",
      },
    },
  ],
});

describe("bitwardenJsonToEntries", () => {
  it("maps every kind the export carries and skips only the unknown", () => {
    const { entries, skipped } = bitwardenJsonToEntries(sample, () => 1);
    expect(skipped).toBe(1);
    expect(entries).toHaveLength(5);

    const note = entries.find((e) => e.type === "note")!;
    expect(note.service).toBe("Just a note");
    expect(note.notes).toBe("text");

    const login = entries.find((e) => e.service === "GitHub")!;
    expect(login.type).toBeUndefined();
    expect(login.password).toBe("hunter2");
    expect(login.totp_secret).toBe("JBSWY3DPEHPK3PXP");
    expect(login.category).toBe("Work");

    const card = entries.find((e) => e.type === "card")!;
    expect(card.card_number).toBe("4111111111111111");
    expect(card.card_code).toBe("123");

    const identity = entries.find((e) => e.type === "identity")!;
    expect(identity.id_full_name).toBe("Alex Stanciu");
    // Fields with no slot of ours survive in notes instead of vanishing.
    expect(identity.notes).toContain("123-45-6789");

    const ssh = entries.find((e) => e.type === "ssh_key")!;
    expect(ssh.ssh_fingerprint).toBe("SHA256:abc");
  });

  it("refuses a password-protected export with advice, not a parse error", () => {
    expect(() => bitwardenJsonToEntries(JSON.stringify({ encrypted: true, items: [] }))).toThrow(
      JsonImportError
    );
  });

  it("rejects JSON that is not a Bitwarden export", () => {
    expect(() => bitwardenJsonToEntries(JSON.stringify({ hello: 1 }))).toThrow(JsonImportError);
  });
});
