import { describe, expect, it } from "vitest";
import { securityKeyDisplayName, usableHere } from "./keyName";
import type { SecurityKeyInfo } from "./types";

const key = (extra: Partial<SecurityKeyInfo>): SecurityKeyInfo => ({
  credential_id: "aa11",
  public_key: "3059",
  key_slot: 1,
  rp_id: "silentsilo.com",
  label: "",
  wrapped_dek: "ef",
  platform: false,
  ...extra,
});

describe("usableHere", () => {
  it("takes the backend's answer over the kind", () => {
    // A fido2 key whose id core refuses, and a Mac key on a Mac build.
    expect(usableHere(key({ kind: "fido2", usable: false }))).toBe(false);
    expect(usableHere(key({ kind: "secure-enclave", usable: true }))).toBe(true);
  });

  it("falls back to the kind when the backend does not say", () => {
    expect(usableHere(key({}))).toBe(true);
    expect(usableHere(key({ kind: "android-keystore" }))).toBe(false);
  });

  it("names a key this computer cannot use as another device's", () => {
    expect(securityKeyDisplayName(key({ kind: "android-keystore", usable: false }))).toBe(
      "Key from another device",
    );
  });
});
