import { afterEach, describe, expect, it } from "vitest";
import { describeFidoPrompt } from "./fidoPrompt";
import { setLanguage } from "../i18n";

describe("describeFidoPrompt", () => {
  afterEach(() => setLanguage("en"));

  it("matches the backend's English", () => {
    expect(
      describeFidoPrompt(
        { code: "unlock_built_in", params: {}, text: "x" },
        "Windows Hello",
      ),
    ).toBe("Confirm with Windows Hello to unlock the silo.");
    expect(
      describeFidoPrompt(
        { code: "verify_key", params: { purpose: "fill", label: "GitHub", site: "github.com" }, text: "x" },
        "Windows Hello",
      ),
    ).toBe("Touch your security key to fill your GitHub login on github.com.");
  });

  it("speaks the language in use", () => {
    setLanguage("ro");
    expect(
      describeFidoPrompt({ code: "keep_key", params: { label: "YubiKey" }, text: "x" }, "Windows Hello"),
    ).toBe("Folosește „YubiKey” ca să rămână funcțională");
  });

  it("falls back to the backend's text for an unknown code", () => {
    expect(describeFidoPrompt({ code: "new_thing", params: {}, text: "Touch it." }, "Touch ID")).toBe(
      "Touch it.",
    );
  });
});
