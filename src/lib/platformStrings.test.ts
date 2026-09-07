import { describe, expect, it } from "vitest";
import { osOf, platformStrings } from "./platformStrings";

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
    expect(`Security key ready. ${s.osName} will show a native prompt.`).toBe(
      "Security key ready. Windows will show a native prompt.",
    );
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
});
