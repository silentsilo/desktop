import { describe, expect, it } from "vitest";
import { t } from "../i18n";
import {
  builtInHint,
  builtInOrKey,
  keysUnavailable,
  osOf,
  ownPromptHint,
  platformStrings,
} from "./platformStrings";

describe("platformStrings", () => {
  it("names the Windows pieces exactly as the screens always did", () => {
    const s = platformStrings("windows");
    expect(s.builtIn).toBe("Windows Hello");
    expect(s.fileManager).toBe("Windows Explorer");
    expect(s.osName).toBe("Windows");
    expect(s.signIn).toBe("sign in to Windows");
    expect(s.offersPhone).toBe(true);
  });

  it("puts the Mac's own names on a Mac", () => {
    const s = platformStrings("macos");
    expect(s.builtIn).toBe("Touch ID");
    expect(s.fileManager).toBe("Finder");
    expect(s.osName).toBe("macOS");
    expect(s.builtInSetupHint).toContain("System Settings");
    expect(s.autostartHint).toContain("Login Items");
    expect(s.fidoUnavailable).toContain("macOS 13");
    // The QR-code phone option is a Windows WebAuthn feature.
    expect(s.offersPhone).toBe(false);
  });

  it("never says Windows on a Mac, or Mac on Windows", () => {
    const mac = platformStrings("macos");
    for (const v of Object.values(mac)) {
      if (typeof v === "string") expect(v).not.toMatch(/Windows|Explorer|Hello/);
    }
    const win = platformStrings("windows");
    for (const v of Object.values(win)) {
      if (typeof v === "string") expect(v).not.toMatch(/macOS|Finder|Touch ID/);
    }
  });

  it("composes the sentences the unlock screen used to hardcode, byte for byte", () => {
    const s = platformStrings("windows");
    expect(`${s.osName} will show its own prompt.`).toBe("Windows will show its own prompt.");
    expect(`Confirm with ${s.builtIn} to unlock.`).toBe("Confirm with Windows Hello to unlock.");
    expect(`2 items from ${s.fileManager}. Choose where they should go.`).toBe(
      "2 items from Windows Explorer. Choose where they should go.",
    );
  });

  it("treats a backend without the field as Windows", () => {
    expect(osOf(null)).toBe("windows");
    expect(osOf(undefined)).toBe("windows");
    expect(osOf({})).toBe("windows");
    expect(osOf({ os: "macos" })).toBe("macos");
  });

  it("offers no built-in key on Linux, which has none the app uses", () => {
    expect(platformStrings("linux").hasBuiltIn).toBe(false);
    expect(platformStrings("macos").hasBuiltIn).toBe(true);
  });

  it("names any of the silo's keys for a confirmation, whatever kinds it has", () => {
    // Hello is not on every Windows silo, and a phone answers too.
    for (const os of ["windows", "macos", "linux"] as const) {
      expect(builtInOrKey(platformStrings(os))).toBe("one of this silo's keys");
    }
  });

  it("says who asks for the key on each system", () => {
    expect(ownPromptHint(platformStrings("windows"))).toBe("Windows will show its own prompt.");
    // No system dialog for a security key off Windows: the app asks for the PIN.
    expect(ownPromptHint(platformStrings("linux"))).toContain("SilentSilo asks for the key's PIN");
    expect(ownPromptHint(platformStrings("macos"))).toContain("Touch ID asks in a macOS prompt");
    expect(ownPromptHint(platformStrings("linux"))).not.toContain("Linux");
  });

  it("says what to do about the built-in key by the reason the backend gave", () => {
    const win = platformStrings("windows");
    const mac = platformStrings("macos");
    expect(builtInHint(win, "no_tpm")).toBe(t("app.platform_hint_no_tpm"));
    expect(builtInHint(win, "update_windows")).toBe(t("app.platform_hint_update_windows"));
    expect(builtInHint(mac, "unavailable")).toBe(t("app.platform_hint_touchid_unavailable"));
    expect(builtInHint(mac, "absent")).toBe(t("app.platform_hint_no_touchid"));
    // Absent elsewhere, set up later, or a backend that gives no reason.
    expect(builtInHint(win, "absent")).toBe(win.builtInSetupHint);
    expect(builtInHint(win, "not_set_up")).toBe(win.builtInSetupHint);
    expect(builtInHint(mac, "not_set_up")).toBe(mac.builtInSetupHint);
    expect(builtInHint(win, undefined)).toBe(win.builtInSetupHint);
    expect(builtInHint(mac, undefined)).toBe(mac.builtInSetupHint);
  });

  it("tells a Linux key that is plugged in but unreadable apart from no key", () => {
    const linux = platformStrings("linux");
    expect(keysUnavailable(linux, { fido_key_present: true })).toBe(
      t("app.platform_fido_no_access_linux"),
    );
    expect(keysUnavailable(linux, { fido_key_present: false })).toBe(linux.fidoUnavailable);
    for (const os of ["windows", "macos"] as const) {
      const s = platformStrings(os);
      expect(keysUnavailable(s, { fido_key_present: true })).toBe(s.fidoUnavailable);
    }
  });
});

describe("builtInHint where a phone works", () => {
  const win = platformStrings("windows");

  it("does not call a security key the only way in", () => {
    for (const status of ["no_tpm", "update_windows", "not_set_up", "absent"] as const) {
      expect(builtInHint(win, status, false)).toContain("the only way in");
      const withPhone = builtInHint(win, status, true);
      expect(withPhone).not.toContain("only way in");
      expect(withPhone).toContain("phone");
    }
  });
});
