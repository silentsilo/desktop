import { describe, expect, it } from "vitest";
import { pastedImage } from "./qrImage";

/** A paste's data as the webview hands it over: a list of items. */
function paste(items: { kind: string; type: string; file?: File }[]): DataTransfer {
  return {
    items: items.map((i) => ({ kind: i.kind, type: i.type, getAsFile: () => i.file ?? null })),
  } as unknown as DataTransfer;
}

describe("pastedImage", () => {
  it("takes the picture a screenshot paste carries", () => {
    const shot = new File([new Uint8Array([1])], "image.png", { type: "image/png" });
    expect(pastedImage(paste([{ kind: "file", type: "image/png", file: shot }]))).toBe(shot);
  });

  it("leaves pasted text to the field", () => {
    expect(pastedImage(paste([{ kind: "string", type: "text/plain" }]))).toBeNull();
    expect(pastedImage(null)).toBeNull();
  });

  it("ignores a file that is not a picture", () => {
    const doc = new File([new Uint8Array([1])], "a.pdf", { type: "application/pdf" });
    expect(pastedImage(paste([{ kind: "file", type: "application/pdf", file: doc }]))).toBeNull();
  });
});
