/** TOTP (RFC 6238) generation — runs entirely client-side against an
 * already-decrypted silo entry, same trust boundary as the password
 * itself. No network calls, no new backend surface. */

const BASE32_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

export type TotpAlgorithm = "SHA-1" | "SHA-256" | "SHA-512";

export type TotpParams = {
  secret: string; // base32, no padding/spaces
  digits: number;
  period: number;
  algorithm: TotpAlgorithm;
  issuer?: string;
  account?: string;
};

export const DEFAULT_TOTP_DIGITS = 6;
export const DEFAULT_TOTP_PERIOD = 30;
export const DEFAULT_TOTP_ALGORITHM: TotpAlgorithm = "SHA-1";

function base32Decode(input: string): Uint8Array {
  const clean = input.toUpperCase().replace(/[^A-Z2-7]/g, "");
  const bytes: number[] = [];
  let bits = 0;
  let value = 0;

  for (const char of clean) {
    const idx = BASE32_ALPHABET.indexOf(char);
    if (idx === -1) continue;
    value = (value << 5) | idx;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((value >>> bits) & 0xff);
    }
  }
  return new Uint8Array(bytes);
}

/** Normalizes a user-typed secret: uppercase, strip spaces/dashes. Does not
 * validate — invalid characters are simply dropped by base32Decode, which
 * would just produce a wrong (but harmless) code rather than throwing, so
 * callers should sanity-check the round trip if they want to warn the user. */
export function normalizeBase32Secret(input: string): string {
  return input.toUpperCase().replace(/[^A-Z2-7]/g, "");
}

/** Accepts either a bare base32 secret or a full `otpauth://totp/...` URI
 * (as shown by services whose QR code you can't scan, or copy-pasted from
 * a "can't scan? use this text code" fallback link) and returns the
 * parsed TOTP parameters, or null if it's neither. */
export function parseTotpInput(input: string): TotpParams | null {
  const trimmed = input.trim();
  if (!trimmed) return null;

  if (trimmed.toLowerCase().startsWith("otpauth://")) {
    try {
      const url = new URL(trimmed);
      if (url.protocol !== "otpauth:" || url.hostname !== "totp") return null;
      const secret = url.searchParams.get("secret");
      if (!secret) return null;

      const label = decodeURIComponent(url.pathname.replace(/^\/+/, ""));
      const [labelIssuer, labelAccount] = label.includes(":")
        ? label.split(":", 2)
        : [undefined, label];

      const algorithmParam = url.searchParams.get("algorithm")?.toUpperCase();
      const algorithm: TotpAlgorithm =
        algorithmParam === "SHA256"
          ? "SHA-256"
          : algorithmParam === "SHA512"
            ? "SHA-512"
            : DEFAULT_TOTP_ALGORITHM;

      const digits = Number.parseInt(url.searchParams.get("digits") ?? "", 10) || DEFAULT_TOTP_DIGITS;
      const period = Number.parseInt(url.searchParams.get("period") ?? "", 10) || DEFAULT_TOTP_PERIOD;

      return {
        secret: normalizeBase32Secret(secret),
        digits,
        period,
        algorithm,
        issuer: url.searchParams.get("issuer")?.trim() || labelIssuer?.trim() || undefined,
        account: labelAccount?.trim() || undefined,
      };
    } catch {
      return null;
    }
  }

  const secret = normalizeBase32Secret(trimmed);
  if (!secret) return null;
  return {
    secret,
    digits: DEFAULT_TOTP_DIGITS,
    period: DEFAULT_TOTP_PERIOD,
    algorithm: DEFAULT_TOTP_ALGORITHM,
  };
}

/** Seconds remaining in the current TOTP window, for a countdown display. */
export function totpSecondsRemaining(period: number = DEFAULT_TOTP_PERIOD, now: number = Date.now()): number {
  const seconds = Math.floor(now / 1000);
  return period - (seconds % period);
}

/** Computes the current TOTP code. Async because it goes through
 * SubtleCrypto's HMAC — cheap, but not synchronous. */
export async function generateTotp(
  params: Pick<TotpParams, "secret" | "digits" | "period" | "algorithm">,
  now: number = Date.now(),
): Promise<string> {
  const keyBytes = base32Decode(params.secret);
  if (keyBytes.length === 0) return "";

  const counter = Math.floor(Math.floor(now / 1000) / params.period);
  const counterBytes = new ArrayBuffer(8);
  const counterView = new DataView(counterBytes);
  // JS numbers only safely hold 53 bits — split into hi/lo 32-bit halves.
  counterView.setUint32(0, Math.floor(counter / 2 ** 32));
  counterView.setUint32(4, counter >>> 0);

  const cryptoKey = await crypto.subtle.importKey(
    "raw",
    keyBytes,
    { name: "HMAC", hash: params.algorithm },
    false,
    ["sign"],
  );
  const signature = new Uint8Array(await crypto.subtle.sign("HMAC", cryptoKey, counterBytes));

  const offset = signature[signature.length - 1]! & 0x0f;
  const binCode =
    ((signature[offset]! & 0x7f) << 24) |
    ((signature[offset + 1]! & 0xff) << 16) |
    ((signature[offset + 2]! & 0xff) << 8) |
    (signature[offset + 3]! & 0xff);

  const code = (binCode % 10 ** params.digits).toString().padStart(params.digits, "0");
  return code;
}
