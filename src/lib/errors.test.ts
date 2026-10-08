import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";
import { hasKey } from "../i18n";
import { decodeAppError, formatAppError, plainError } from "./errors";

describe("formatAppError", () => {
  it("recognizes an unconfigured bucket as a benign local-only notice", () => {
    const expected = "Not backed up. This silo is only on this computer.";
    expect(formatAppError("CloudNotConfigured")).toBe(expected);
    expect(
      formatAppError("That file is not on this computer, and no backup storage is connected."),
    ).toBe(expected);
  });

  it("maps a security-key cancellation", () => {
    expect(formatAppError("user_cancelled")).toBe("The key prompt was cancelled.");
    expect(formatAppError("Security key operation cancelled")).toBe(
      "The key prompt was cancelled.",
    );
  });

  it("maps a security-key timeout", () => {
    expect(formatAppError("Security key request timed out")).toBe(
      "The key prompt timed out. Try again.",
    );
  });

  it("does not blame the key for a storage timeout or a stopped job", () => {
    expect(formatAppError("request timed out")).toBe(
      "Your backup storage did not answer in time. Check your connection and try again.",
    );
    expect(formatAppError("could not open cancelled-order.pdf: access denied")).toBe(
      "could not open cancelled-order.pdf: access denied",
    );
  });

  it("maps connection failures to backup storage, whatever kind it is", () => {
    const expected = "Cannot reach your backup storage. Check your connection and the address.";
    expect(formatAppError("error sending request for url (...)")).toBe(expected);
    expect(formatAppError("tcp connect error: Connection refused")).toBe(expected);
  });

  it("maps a rejected access key to the storage settings", () => {
    const expected =
      "Your backup storage refused the sign-in. Check the username and password, or the access key.";
    expect(formatAppError("service error: unauthorized")).toBe(expected);
    expect(formatAppError("dispatch failure (403): SignatureDoesNotMatch")).toBe(expected);
  });

  it("maps a missing bucket to something the user can act on", () => {
    expect(formatAppError("service error: NoSuchBucket")).toBe(
      "That bucket does not exist. Check its name and region.",
    );
  });

  it("maps FIDO enrollment-state messages", () => {
    expect(formatAppError("no security key enrolled")).toBe("No key enrolled yet.");
    expect(formatAppError("This key is already enrolled")).toBe("That key is already enrolled.");
    expect(formatAppError("credential already enrolled")).toBe("That key is already enrolled.");
    expect(formatAppError("Keep at least one key enrolled")).toBe(
      "Keep at least one key on the silo.",
    );
    expect(formatAppError("keep at least one security key")).toBe(
      "Keep at least one key on the silo.",
    );
  });

  it("passes through a first-unlock/enrollment-related vault-locked message verbatim", () => {
    const msg = "VaultLocked: unlock and enroll a security key first";
    expect(formatAppError(msg)).toBe(msg);
    expect(formatAppError("Enrol a key before unlocking")).toBe("Enrol a key before unlocking");
  });

  it("strips common Rust/Tauri error-wrapper prefixes for an unrecognized message", () => {
    expect(formatAppError("Error: something odd happened")).toBe("something odd happened");
    expect(formatAppError('invoke(vault_unlock): disk is full')).toBe("disk is full");
  });

  it("falls back to a generic message for empty/unknown input", () => {
    expect(formatAppError("")).toBe("Something went wrong.");
    expect(formatAppError(null)).toBe("Unknown error");
    expect(formatAppError(undefined)).toBe("Unknown error");
  });

  it("returns an unrecognized message as-is once wrapper prefixes are stripped", () => {
    expect(formatAppError("some entirely novel error text")).toBe("some entirely novel error text");
  });
});

describe("rules that used to match too much", () => {
  it("does not read a number inside other text as an HTTP status", () => {
    // "401" appears in file names, key ids and byte counts. Matching it as
    // a substring turned an unrelated failure into advice about storage
    // credentials, which sent people to change settings that were fine.
    expect(formatAppError("could not import 401k-statement.pdf")).toBe(
      "could not import 401k-statement.pdf",
    );
    expect(formatAppError("wrote 1403 bytes")).toBe("wrote 1403 bytes");
  });

  it("still reads a real status", () => {
    expect(formatAppError("dispatch failure (403): SignatureDoesNotMatch")).toBe(
      "Your backup storage refused the sign-in. Check the username and password, or the access key.",
    );
  });

  it("does not claim every sentence containing 'at least one'", () => {
    expect(formatAppError("pick at least one folder to export")).toBe(
      "pick at least one folder to export",
    );
    expect(formatAppError("Keep at least one key enrolled")).toBe(
      "Keep at least one key on the silo.",
    );
  });

  it("passes an unlock instruction through instead of rewriting it", () => {
    // The branch that used to fall through to the cancellation rule, which
    // then replaced a useful instruction with "Security key prompt was
    // cancelled."
    expect(formatAppError("Enroll a security key before unlocking")).toBe(
      "Enroll a security key before unlocking",
    );
  });
});

describe("coded backend errors", () => {
  const SEP = "\u001f";

  it("are said by their key, with their values", () => {
    expect(formatAppError(`That key was not found.${SEP}err.key_not_found`)).toBe(
      "That key was not found.",
    );
    const taken = `You already have a silo called “Work”.${SEP}err.silo_name_taken${SEP}{"name":"Work"}`;
    expect(decodeAppError(taken)).toEqual({
      message: "You already have a silo called “Work”.",
      code: "err.silo_name_taken",
      params: { name: "Work" },
    });
    expect(formatAppError(taken)).toBe("You already have a silo called “Work”.");
  });

  it("fall back to their English when this build has no text for the key", () => {
    expect(formatAppError(`Something new.${SEP}err.not_in_this_build`)).toBe("Something new.");
  });

  it("leave plain strings as they were", () => {
    expect(decodeAppError("cancelled")).toEqual({ message: "cancelled", code: null, params: {} });
  });

  it("lose their keys inside a list the backend built", () => {
    expect(plainError(`a.txt: Not a file.${SEP}err.not_a_file; b: x${SEP}err.y${SEP}{"p":"1"}`)).toBe(
      "a.txt: Not a file.; b: x",
    );
  });

  it("have a text for every key the backend sends", () => {
    const root = path.resolve(__dirname, "../../src-tauri/src");
    const codes = new Set<string>();
    const walk = (dir: string) => {
      for (const name of fs.readdirSync(dir)) {
        const full = path.join(dir, name);
        if (fs.statSync(full).isDirectory()) walk(full);
        else if (full.endsWith(".rs")) {
          const text = fs.readFileSync(full, "utf8");
          for (const m of text.matchAll(/coded!\(\s*"(err\.[\w.]+)"/g)) codes.add(m[1]!);
          for (const m of text.matchAll(/coded_with\(\s*"(err\.[\w.]+)"/g)) codes.add(m[1]!);
          // Written by hand where a crate cannot reach the helper.
          for (const m of text.matchAll(/\\u\{1f\}(err\.[\w.]+)/g)) codes.add(m[1]!);
        }
      }
    };
    walk(root);
    // Core's own, when its checkout sits beside this one (on a developer's
    // machine; CI has only the pinned tag, under cargo's own directory).
    const core = path.resolve(__dirname, "../../../silentsilo.core/crates");
    if (fs.existsSync(core)) walk(core);
    expect(codes.size).toBeGreaterThan(50);
    expect([...codes].filter((code) => !hasKey(code))).toEqual([]);
  });
});
