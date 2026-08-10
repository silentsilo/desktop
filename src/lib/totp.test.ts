import { describe, expect, it } from "vitest";
import {
  DEFAULT_TOTP_ALGORITHM,
  DEFAULT_TOTP_DIGITS,
  DEFAULT_TOTP_PERIOD,
  generateTotp,
  normalizeBase32Secret,
  parseTotpInput,
  totpSecondsRemaining,
} from "./totp";

// RFC 6238 Appendix B official test vectors. The secret is the ASCII string
// "12345678901234567890" (repeated/truncated per the RFC for the SHA-256 and
// SHA-512 vectors), base32-encoded. Hand-verified once already this session
// via a throwaway script — this is that verification turned into a
// permanent regression test instead of a one-off.
function asciiToBase32(str: string): string {
  const BASE32_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  const bytes = new TextEncoder().encode(str);
  let bits = 0;
  let value = 0;
  let output = "";
  for (const b of bytes) {
    value = (value << 8) | b;
    bits += 8;
    while (bits >= 5) {
      bits -= 5;
      output += BASE32_ALPHABET[(value >>> bits) & 31];
    }
  }
  if (bits > 0) output += BASE32_ALPHABET[(value << (5 - bits)) & 31];
  return output;
}

const SECRET_SHA1 = asciiToBase32("12345678901234567890");
const SECRET_SHA256 = asciiToBase32("12345678901234567890123456789012");
const SECRET_SHA512 = asciiToBase32(
  "1234567890123456789012345678901234567890123456789012345678901234",
);

describe("generateTotp — RFC 6238 Appendix B vectors", () => {
  const vectors: { time: number; sha1: string; sha256: string; sha512: string }[] = [
    { time: 59, sha1: "94287082", sha256: "46119246", sha512: "90693936" },
    { time: 1111111109, sha1: "07081804", sha256: "68084774", sha512: "25091201" },
    { time: 1111111111, sha1: "14050471", sha256: "67062674", sha512: "99943326" },
    { time: 1234567890, sha1: "89005924", sha256: "91819424", sha512: "93441116" },
    { time: 2000000000, sha1: "69279037", sha256: "90698825", sha512: "38618901" },
  ];

  for (const v of vectors) {
    it(`matches the official vector at t=${v.time} (SHA-1)`, async () => {
      const code = await generateTotp(
        { secret: SECRET_SHA1, digits: 8, period: 30, algorithm: "SHA-1" },
        v.time * 1000,
      );
      expect(code).toBe(v.sha1);
    });

    it(`matches the official vector at t=${v.time} (SHA-256)`, async () => {
      const code = await generateTotp(
        { secret: SECRET_SHA256, digits: 8, period: 30, algorithm: "SHA-256" },
        v.time * 1000,
      );
      expect(code).toBe(v.sha256);
    });

    it(`matches the official vector at t=${v.time} (SHA-512)`, async () => {
      const code = await generateTotp(
        { secret: SECRET_SHA512, digits: 8, period: 30, algorithm: "SHA-512" },
        v.time * 1000,
      );
      expect(code).toBe(v.sha512);
    });
  }

  it("returns an empty string for an empty/unusable secret", async () => {
    const code = await generateTotp(
      { secret: "", digits: 6, period: 30, algorithm: "SHA-1" },
      Date.now(),
    );
    expect(code).toBe("");
  });

  it("pads shorter codes with leading zeros", async () => {
    // Any of the 6-digit defaults could legitimately start with a zero —
    // just confirm the length is always exactly `digits`, never shorter.
    const code = await generateTotp(
      { secret: SECRET_SHA1, digits: 6, period: 30, algorithm: "SHA-1" },
      59 * 1000,
    );
    expect(code).toHaveLength(6);
  });
});

describe("parseTotpInput", () => {
  it("parses a bare base32 secret with the documented defaults", () => {
    const parsed = parseTotpInput("JBSWY3DPEHPK3PXP");
    expect(parsed).toEqual({
      secret: "JBSWY3DPEHPK3PXP",
      digits: DEFAULT_TOTP_DIGITS,
      period: DEFAULT_TOTP_PERIOD,
      algorithm: DEFAULT_TOTP_ALGORITHM,
    });
  });

  it("normalizes lowercase and strips spaces/dashes from a bare secret", () => {
    const parsed = parseTotpInput("jbsw y3dp-ehpk 3pxp");
    expect(parsed?.secret).toBe("JBSWY3DPEHPK3PXP");
  });

  it("parses a full otpauth:// URI with issuer/account/algorithm/digits/period", () => {
    const uri =
      "otpauth://totp/Example:alice@example.com?secret=JBSWY3DPEHPK3PXP&issuer=Example&algorithm=SHA256&digits=8&period=60";
    const parsed = parseTotpInput(uri);
    expect(parsed).toEqual({
      secret: "JBSWY3DPEHPK3PXP",
      digits: 8,
      period: 60,
      algorithm: "SHA-256",
      issuer: "Example",
      account: "alice@example.com",
    });
  });

  it("falls back to otpauth:// defaults when digits/period/algorithm are omitted", () => {
    const uri = "otpauth://totp/Example:alice@example.com?secret=JBSWY3DPEHPK3PXP";
    const parsed = parseTotpInput(uri);
    expect(parsed?.digits).toBe(DEFAULT_TOTP_DIGITS);
    expect(parsed?.period).toBe(DEFAULT_TOTP_PERIOD);
    expect(parsed?.algorithm).toBe(DEFAULT_TOTP_ALGORITHM);
  });

  it("rejects an otpauth:// URI missing a secret", () => {
    expect(parseTotpInput("otpauth://totp/Example:alice@example.com?issuer=Example")).toBeNull();
  });

  it("rejects a non-totp otpauth:// URI", () => {
    expect(parseTotpInput("otpauth://hotp/Example:alice@example.com?secret=ABC")).toBeNull();
  });

  it("rejects malformed input", () => {
    expect(parseTotpInput("otpauth://totp/not a valid url [[[")).toBeNull();
  });

  it("rejects empty input", () => {
    expect(parseTotpInput("")).toBeNull();
    expect(parseTotpInput("   ")).toBeNull();
  });

  it("rejects a bare secret that normalizes to nothing", () => {
    expect(parseTotpInput("!!!@@@###")).toBeNull();
  });
});

describe("normalizeBase32Secret", () => {
  it("uppercases and strips non-base32 characters", () => {
    expect(normalizeBase32Secret("jbsw-y3dp 3pxp!")).toBe("JBSWY3DP3PXP");
  });
});

describe("totpSecondsRemaining", () => {
  it("counts down within a period and wraps at the boundary", () => {
    expect(totpSecondsRemaining(30, 0)).toBe(30);
    expect(totpSecondsRemaining(30, 1000)).toBe(29);
    expect(totpSecondsRemaining(30, 29_000)).toBe(1);
    expect(totpSecondsRemaining(30, 30_000)).toBe(30);
  });
});

describe("otpauth parameters are user-supplied text", () => {
  it("keeps the whole account when the label carries more colons", () => {
    // `split(":", 2)` truncates rather than limiting the cuts, so this used
    // to come back as "user" and quietly drop the rest of the name.
    const params = parseTotpInput(
      "otpauth://totp/Example:user:name?secret=JBSWY3DPEHPK3PXP",
    );
    expect(params?.issuer).toBe("Example");
    expect(params?.account).toBe("user:name");
  });

  it("still reads a label with no issuer", () => {
    const params = parseTotpInput("otpauth://totp/alice@example.com?secret=JBSWY3DPEHPK3PXP");
    expect(params?.account).toBe("alice@example.com");
    expect(params?.issuer).toBeUndefined();
  });

  it("refuses a digit count no code can have", () => {
    // `10 ** 100` is Infinity, and the modulo against it returns the raw
    // value padded to a hundred characters.
    expect(parseTotpInput("otpauth://totp/a?secret=JBSWY3DPEHPK3PXP&digits=100")?.digits).toBe(6);
    expect(parseTotpInput("otpauth://totp/a?secret=JBSWY3DPEHPK3PXP&digits=-4")?.digits).toBe(6);
    expect(parseTotpInput("otpauth://totp/a?secret=JBSWY3DPEHPK3PXP&digits=8")?.digits).toBe(8);
  });

  it("refuses a period that would divide by zero or freeze the counter", () => {
    expect(parseTotpInput("otpauth://totp/a?secret=JBSWY3DPEHPK3PXP&period=0")?.period).toBe(30);
    expect(parseTotpInput("otpauth://totp/a?secret=JBSWY3DPEHPK3PXP&period=99999")?.period).toBe(30);
    expect(parseTotpInput("otpauth://totp/a?secret=JBSWY3DPEHPK3PXP&period=60")?.period).toBe(60);
  });
});
