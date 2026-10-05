import { describe, expect, it } from "vitest";
import { attachmentBlobs, kdbxToEntries, type KdbxEntry, type KdbxVersion } from "./kdbx";

function version(over: Partial<KdbxVersion> = {}): KdbxVersion {
  return {
    title: "Bank",
    username: "ana",
    password: "now",
    url: "https://bank.example",
    notes: "",
    otp: null,
    fields: [],
    modified: 2000,
    ...over,
  };
}

function kp(over: Partial<KdbxEntry> = {}): KdbxEntry {
  return {
    ...version(),
    group: ["Money", "Main"],
    tags: [],
    created: 1000,
    attachments: [],
    history: [],
    ...over,
  };
}

describe("kdbxToEntries", () => {
  it("files an entry under its whole group path", () => {
    const { entries } = kdbxToEntries([kp()], 10, () => 9);
    expect(entries[0]).toMatchObject({
      service: "Bank",
      username: "ana",
      password: "now",
      category: "Money / Main",
      created_at: 1000,
      updated_at: 2000,
      type: "login",
    });
  });

  it("files an entry at the top as General", () => {
    expect(kdbxToEntries([kp({ group: [] })], 10).entries[0].category).toBe("General");
  });

  it("reads a KeePassXC otp URI as the two-factor code", () => {
    const { entries } = kdbxToEntries(
      [kp({ otp: "otpauth://totp/Bank:ana?secret=JBSWY3DPEHPK3PXP&digits=8&period=60" })],
      10,
    );
    expect(entries[0]).toMatchObject({
      totp_secret: "JBSWY3DPEHPK3PXP",
      totp_digits: 8,
      totp_period: 60,
    });
  });

  it("keeps a two-factor secret it cannot read in notes", () => {
    const { entries } = kdbxToEntries([kp({ otp: "steam://ABCDE" })], 10);
    expect(entries[0].totp_secret).toBeUndefined();
    expect(entries[0].notes).toContain("Two-factor secret: steam://ABCDE");
  });

  it("brings extra fields in as custom fields, protected ones hidden", () => {
    const { entries } = kdbxToEntries(
      [
        kp({
          fields: [
            { name: "Customer number", value: "40021", protected: false },
            { name: "PIN", value: "1234", protected: true },
          ],
        }),
      ],
      10,
    );
    expect(entries[0].fields).toEqual([
      { name: "Customer number", value: "40021", hidden: false },
      { name: "PIN", value: "1234", hidden: true },
    ]);
  });

  it("stars an entry this app exported as starred, and keeps other tags in notes", () => {
    const { entries } = kdbxToEntries([kp({ tags: ["Favorite", "work", "vpn"] })], 10);
    expect(entries[0].favorite).toBe(true);
    expect(entries[0].notes).toBe("Tags: work, vpn");
  });

  it("brings the history in, newest first, cut to the setting", () => {
    const history = [3, 2, 1].map((n) => version({ password: `p${n}`, modified: n * 100 }));
    const { entries } = kdbxToEntries([kp({ history })], 2);
    expect(entries[0].history?.map((v) => [v.password, v.saved_at])).toEqual([
      ["p3", 300],
      ["p2", 200],
    ]);
    // A version holds what the entry said, not where it was filed.
    expect(entries[0].history?.[0]).not.toHaveProperty("category");
    expect(entries[0].history?.[0]).not.toHaveProperty("id");
  });

  it("names an untitled entry after its address and skips an empty one", () => {
    const { entries, skipped } = kdbxToEntries(
      [kp({ title: "" }), kp({ title: "", username: "", password: "", url: "" })],
      10,
    );
    expect(entries.map((e) => e.service)).toEqual(["https://bank.example"]);
    expect(skipped).toHaveLength(1);
  });

  it("lists the attachments an import made, for a cancel to delete", () => {
    const attachment = { blob_id: "b1", name: "codes.txt", size_bytes: 6, blob_key: "k" };
    const { entries } = kdbxToEntries([kp({ attachments: [attachment] })], 10);
    expect(entries[0].attachments).toEqual([attachment]);
    expect(attachmentBlobs(entries)).toEqual(["b1"]);
  });
});
