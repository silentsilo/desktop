/** Connection details as the Settings form holds them. */
export type S3Form = {
  endpoint: string;
  region: string;
  bucket: string;
  prefix: string;
  accessKeyId: string;
  /** Blank means "keep whatever is already stored". */
  secretAccessKey: string;
  pathStyle: boolean;
};

export type S3Preset = {
  id: string;
  label: string;
  /** `{}` placeholders the user has to fill in; kept literal so the field
   * shows what shape the endpoint takes rather than an empty box. */
  endpoint: string;
  region: string;
  pathStyle: boolean;
  hint: string;
};

/** The providers worth one-click support. `custom` is the escape hatch —
 * anything speaking S3 works, these just spare the user a docs lookup for
 * the two settings that are easy to get wrong (endpoint shape and whether
 * path-style addressing is required). */
export const S3_PRESETS: S3Preset[] = [
  {
    id: "backblaze",
    label: "Backblaze B2",
    endpoint: "https://s3.us-west-004.backblazeb2.com",
    region: "us-west-004",
    pathStyle: true,
    hint: "Endpoint and region both carry your bucket's region code. Copy them from the bucket's details page.",
  },
  {
    id: "r2",
    label: "Cloudflare R2",
    endpoint: "https://<account-id>.r2.cloudflarestorage.com",
    region: "auto",
    pathStyle: false,
    hint: "Replace <account-id> with yours. R2 ignores region, so leave it as auto.",
  },
  {
    id: "wasabi",
    label: "Wasabi",
    endpoint: "https://s3.eu-central-1.wasabisys.com",
    region: "eu-central-1",
    pathStyle: false,
    hint: "Both the endpoint and region must match the region your bucket lives in.",
  },
  {
    id: "minio",
    label: "MinIO (self-hosted)",
    endpoint: "http://localhost:9000",
    region: "us-east-1",
    pathStyle: true,
    hint: "MinIO needs path-style addressing. Region can be anything, it isn't used.",
  },
  {
    id: "aws",
    label: "Amazon S3",
    endpoint: "https://s3.eu-central-1.amazonaws.com",
    region: "eu-central-1",
    pathStyle: false,
    hint: "Use the regional endpoint for the bucket, not the global one.",
  },
  {
    id: "custom",
    label: "Other S3-compatible",
    endpoint: "",
    region: "us-east-1",
    pathStyle: true,
    // The path-style advice lives on the checkbox itself, which is where
    // someone acts on it; saying it here as well printed it twice on one
    // screen.
    hint: "Any provider speaking the S3 API works.",
  },
];

export const DEFAULT_S3_FORM: S3Form = {
  endpoint: "",
  region: "us-east-1",
  bucket: "",
  prefix: "silentsilo",
  accessKeyId: "",
  secretAccessKey: "",
  pathStyle: true,
};

export function applyPreset(form: S3Form, preset: S3Preset): S3Form {
  return { ...form, endpoint: preset.endpoint, region: preset.region, pathStyle: preset.pathStyle };
}

/** Which preset a stored config most likely came from, so reopening
 * Settings shows the right one selected instead of always "Other". */
export function detectPreset(endpoint: string): S3Preset {
  const host = endpoint.trim().toLowerCase();
  const match = (needle: string) => host.includes(needle);

  if (match("backblazeb2.com")) return S3_PRESETS[0]!;
  if (match("r2.cloudflarestorage.com")) return S3_PRESETS[1]!;
  if (match("wasabisys.com")) return S3_PRESETS[2]!;
  if (match("amazonaws.com")) return S3_PRESETS[4]!;
  return S3_PRESETS[5]!;
}

/** Fields the user must supply before a save is worth attempting.
 * `secretAccessKey` is deliberately absent — blank means "keep the stored
 * one", which is valid when editing an existing connection. */
export function missingFields(form: S3Form, hasStoredSecret: boolean): string[] {
  const missing: string[] = [];
  if (!form.endpoint.trim()) missing.push("Endpoint");
  if (!form.bucket.trim()) missing.push("Bucket");
  if (!form.accessKeyId.trim()) missing.push("Access key ID");
  if (!form.secretAccessKey.trim() && !hasStoredSecret) missing.push("Secret access key");
  return missing;
}
