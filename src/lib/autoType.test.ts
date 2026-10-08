import { describe, expect, it } from "vitest";
import { matchScore, rankForAutoType } from "./autoType";
import type { PasswordEntry } from "./types";

function login(service: string, url = "", over: Partial<PasswordEntry> = {}): PasswordEntry {
  return {
    id: service,
    service,
    username: "me",
    password: "pw",
    url,
    notes: "",
    category: "General",
    created_at: 0,
    updated_at: 0,
    ...over,
  };
}

describe("auto-type matching", () => {
  it("ranks the login named in the window first", () => {
    const ranked = rankForAutoType(
      [login("Bank"), login("Steam", "https://store.steampowered.com"), login("GitHub")],
      { title: "Steam - Sign In", program: "steam.exe" },
    );
    expect(ranked[0]!.entry.service).toBe("Steam");
    expect(ranked[0]!.score).toBeGreaterThan(0);
    expect(ranked.slice(1).map((r) => r.score)).toEqual([0, 0]);
  });

  it("matches by the site's name when the entry is named otherwise", () => {
    expect(
      matchScore(login("Work VPN", "https://vpn.example.org"), {
        title: "Example VPN Client",
        program: "vpnui.exe",
      }),
    ).toBeGreaterThan(0);
  });

  it("offers only logins with a password, the rest by name", () => {
    const ranked = rankForAutoType(
      [login("Zeta"), login("Alpha"), login("Card", "", { type: "card" }), login("Empty", "", { password: "" })],
      { title: "Untitled", program: "notepad.exe" },
    );
    expect(ranked.map((r) => r.entry.service)).toEqual(["Alpha", "Zeta"]);
  });

  it("does not match on words every window has", () => {
    expect(matchScore(login("Login"), { title: "Sign in - Windows", program: "app.exe" })).toBe(0);
  });
});
