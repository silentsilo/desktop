import { describe, expect, it } from "vitest";
import { describeFidoPrompt } from "./fidoPrompt";

const verify = (code: string, purpose: string, extra: Record<string, string> = {}) => ({
  code,
  params: { purpose, ...extra },
  text: "",
});

describe("adding a phone, worded by what the silo already has", () => {
  it("a security key or a phone: touch the key, or scan with that phone, not the new one", () => {
    const said = describeFidoPrompt(
      verify("verify_key", "key_add", { phone: "1", adding_phone: "key" }),
      "Windows Hello",
    );
    expect(said).toContain("Touch the security key this silo already has");
    expect(said).toContain("Not the new phone yet");
  });

  it("Hello and removable keys: names all three", () => {
    const said = describeFidoPrompt(
      verify("verify_key", "key_add", { phone: "1", adding_phone: "any" }),
      "Windows Hello",
    );
    expect(said).toContain("Windows Hello, its security key, or the phone added before");
  });

  it("Hello only: confirm with Hello, the new phone comes next", () => {
    const said = describeFidoPrompt(
      verify("verify_built_in", "key_add", { adding_phone: "builtin" }),
      "Windows Hello",
    );
    expect(said).toContain("Windows Hello");
    expect(said).toContain("The new phone comes next");
    expect(said).not.toContain("QR code: scan");
  });

  it("then names the new phone at its QR code", () => {
    const said = describeFidoPrompt({ code: "enrol_phone_new", params: {}, text: "" }, "");
    expect(said).toContain("new phone");
  });

  it("does not speak of a new phone when a security key is added", () => {
    const said = describeFidoPrompt(
      verify("verify_key", "key_add", { phone: "1" }),
      "Windows Hello",
    );
    expect(said).toContain("already added to this silo");
    expect(said).not.toContain("new phone");
  });

  it("says nothing about phones where none can be used", () => {
    const said = describeFidoPrompt(verify("verify_key", "key_add"), "Windows Hello");
    expect(said).not.toContain("phone");
  });
});
