import { describe, expect, it } from "vitest";
import {
  applyPreset,
  DEFAULT_S3_FORM,
  detectPreset,
  missingFields,
  S3_PRESETS,
  type S3Form,
} from "./s3Presets";

function form(overrides: Partial<S3Form> = {}): S3Form {
  return {
    ...DEFAULT_S3_FORM,
    endpoint: "https://s3.example.com",
    bucket: "my-bucket",
    accessKeyId: "AKIAEXAMPLE",
    secretAccessKey: "secret",
    ...overrides,
  };
}

describe("presets", () => {
  it("every preset has a usable id and label", () => {
    for (const preset of S3_PRESETS) {
      expect(preset.id).toBeTruthy();
      expect(preset.label).toBeTruthy();
      expect(preset.hint).toBeTruthy();
    }
  });

  it("only MinIO and the custom fallback default to path-style", () => {
    // Getting this backwards is the single most common cause of "it just
    // won't connect", so it's worth pinning: MinIO requires path-style,
    // the hosted providers all use virtual-host addressing.
    const pathStyle = S3_PRESETS.filter((p) => p.pathStyle).map((p) => p.id);
    expect(pathStyle).toEqual(["backblaze", "minio", "custom"]);
  });

  it("applying a preset replaces only the provider-specific fields", () => {
    const original = form({ bucket: "keep-me", prefix: "keep-this", accessKeyId: "keep-key" });
    const minio = S3_PRESETS.find((p) => p.id === "minio")!;
    const next = applyPreset(original, minio);

    expect(next.endpoint).toBe(minio.endpoint);
    expect(next.region).toBe(minio.region);
    expect(next.pathStyle).toBe(true);
    // Anything the user typed themselves survives switching provider.
    expect(next.bucket).toBe("keep-me");
    expect(next.prefix).toBe("keep-this");
    expect(next.accessKeyId).toBe("keep-key");
    expect(next.secretAccessKey).toBe(original.secretAccessKey);
  });
});

describe("detectPreset", () => {
  it.each([
    ["https://s3.us-west-004.backblazeb2.com", "backblaze"],
    ["https://abc123.r2.cloudflarestorage.com", "r2"],
    ["https://s3.eu-central-1.wasabisys.com", "wasabi"],
    ["https://s3.eu-central-1.amazonaws.com", "aws"],
    ["http://localhost:9000", "custom"],
    ["https://storage.mycompany.internal", "custom"],
  ])("maps %s to %s", (endpoint, expected) => {
    expect(detectPreset(endpoint).id).toBe(expected);
  });

  it("ignores case and surrounding whitespace", () => {
    expect(detectPreset("  HTTPS://S3.US-WEST-004.BackblazeB2.com  ").id).toBe("backblaze");
  });

  it("falls back to custom for an empty endpoint", () => {
    expect(detectPreset("").id).toBe("custom");
  });
});

describe("missingFields", () => {
  it("passes a complete form", () => {
    expect(missingFields(form(), false)).toEqual([]);
  });

  it("names each blank required field", () => {
    const empty = form({ endpoint: "", bucket: "", accessKeyId: "", secretAccessKey: "" });
    expect(missingFields(empty, false)).toEqual([
      "Endpoint",
      "Bucket",
      "Access key ID",
      "Secret access key",
    ]);
  });

  it("treats whitespace as blank", () => {
    expect(missingFields(form({ bucket: "   " }), false)).toEqual(["Bucket"]);
  });

  it("accepts a blank secret when one is already stored", () => {
    // Editing just the prefix shouldn't force retyping the secret, which
    // the UI never shows back in the first place.
    expect(missingFields(form({ secretAccessKey: "" }), true)).toEqual([]);
  });

  it("still requires a secret when none is stored yet", () => {
    expect(missingFields(form({ secretAccessKey: "" }), false)).toEqual(["Secret access key"]);
  });

  it("does not require a prefix — the bucket root is valid", () => {
    expect(missingFields(form({ prefix: "" }), false)).toEqual([]);
  });
});
