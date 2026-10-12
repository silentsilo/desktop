import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { fromOurReleases, signedForVersion } from "./updater";

const AT = "https://github.com/silentsilo/desktop/releases/download/";

describe("an update is offered only from SilentSilo's releases", () => {
  it("under the tag of the version it claims", () => {
    const raw = {
      version: "1.5.0",
      platforms: { "windows-x86_64": { url: `${AT}v1.5.0/SilentSilo_1.5.0_x64-setup.exe` } },
    };
    expect(fromOurReleases("1.5.0", raw)).toBe(true);
    // An older installer offered as a newer version.
    expect(fromOurReleases("9.9.9", raw)).toBe(false);
  });

  it("and from nowhere else", () => {
    const at = (url: string) => ({ platforms: { "windows-x86_64": { url } } });
    expect(fromOurReleases("1.5.0", at("https://example.com/v1.5.0/setup.exe"))).toBe(false);
    expect(
      fromOurReleases(
        "1.5.0",
        at("https://github.com/someone/desktop/releases/download/v1.5.0/x.exe"),
      ),
    ).toBe(false);
    expect(fromOurReleases("1.5.0", at(`${AT}v1.5.0/../v1.0.0/x.exe`))).toBe(false);
    expect(fromOurReleases("1.5.0", at(`${AT}v1.5.0/..%2Fv1.0.0%2Fx.exe`))).toBe(false);
    expect(fromOurReleases("1.5.0", {})).toBe(false);
  });

  it("every platform it names", () => {
    const raw = {
      platforms: {
        "windows-x86_64": { url: `${AT}v1.5.0/a.exe` },
        "linux-x86_64-deb": { url: "https://example.com/b.deb" },
      },
    };
    expect(fromOurReleases("1.5.0", raw)).toBe(false);
  });
});

describe("signedForVersion", () => {
  const sig = (comment: string) =>
    btoa(
      "untrusted comment: signature from rsign secret key\nRUSxx\n" +
        `trusted comment: ${comment}\nZZZZ\n`,
    );
  const answer = (comment: string) => ({
    version: "1.5.1",
    platforms: { "windows-x86_64": { signature: sig(comment), url: "x" } },
  });

  it("takes a signature that names the version offered", () => {
    expect(
      signedForVersion("1.5.1", answer("version:1.5.1\tfile:SilentSilo_1.5.1_x64-setup.exe")),
    ).toBe(true);
  });

  it("refuses one made for another version", () => {
    expect(
      signedForVersion("1.5.1", answer("version:1.4.0\tfile:SilentSilo_1.4.0_x64-setup.exe")),
    ).toBe(false);
  });

  it("refuses one that names no version, as the old signer wrote", () => {
    expect(
      signedForVersion("1.5.1", answer("timestamp:1\tfile:SilentSilo_1.5.1_x64-setup.exe")),
    ).toBe(false);
  });

  it("refuses an answer with no signature at all", () => {
    expect(signedForVersion("1.5.1", { version: "1.5.1", platforms: {} })).toBe(false);
  });
});
