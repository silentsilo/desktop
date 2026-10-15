import { describe, expect, it } from "vitest";
import { setupReportUrl } from "./setupReport";

const bootstrap = {
  os: "windows" as const,
  fido_available: true,
  fido_key_present: false,
  platform_authenticator: false,
  platform_status: "update_windows" as const,
  phone_available: true,
};

describe("setupReportUrl", () => {
  it("opens a new issue on the desktop repository", () => {
    const url = new URL(setupReportUrl(bootstrap, "1.5.1"));
    expect(url.origin + url.pathname).toBe("https://github.com/silentsilo/desktop/issues/new");
    expect(url.searchParams.get("title")).toBe("Setup did not work");
  });

  it("carries what the platform offers and nothing else", () => {
    const body = new URL(setupReportUrl(bootstrap, "1.5.1")).searchParams.get("body") ?? "";
    expect(body).toContain("App: 1.5.1");
    expect(body).toContain("System: windows");
    expect(body).toContain("Built-in (Windows Hello or Touch ID): update_windows");
    expect(body).toContain("Phone over QR code: available");
    // Only these fields: a silo name, a path or a key id would be one more
    // line, and the body has a fixed shape.
    const facts = body.split("---")[1].trim().split("\n");
    expect(facts).toHaveLength(5);
  });
});
