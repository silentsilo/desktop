import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { fromOurReleases } from "./updater";

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
