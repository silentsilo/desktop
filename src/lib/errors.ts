import { hasKey, t } from "../i18n";

/** What separates a backend error's English from its code (`err.rs`). */
const SEP = "\u001f";

export type DecodedError = {
  /** The English the backend wrote, as a log shows it. */
  message: string;
  /** The catalog key it is translated by, when the backend gave one. */
  code: string | null;
  params: Record<string, string>;
};

/** Text the backend built around errors (a list of failed items, "name:
 * reason"), with each coded error's key taken out so only the English
 * shows. Whole errors go through `formatAppError`, which translates. */
export function plainError(text: string): string {
  return text.replace(CODE_TAIL, "");
}

/** A key and, when it has values, their JSON, each after the separator. */
const CODE_TAIL = new RegExp(String.raw`${SEP}[\w.]+(${SEP}\{[^}]*\})?`, "g");

/** Splits a backend error into its English and its code. A plain string
 * comes back as the message alone. */
export function decodeAppError(err: unknown): DecodedError {
  const raw = String(err ?? "");
  const at = raw.indexOf(SEP);
  if (at < 0) return { message: raw, code: null, params: {} };
  const [code = "", json = ""] = raw.slice(at + 1).split(SEP);
  let params: Record<string, string> = {};
  if (json) {
    try {
      params = JSON.parse(json) as Record<string, string>;
    } catch {
      params = {};
    }
  }
  return { message: raw.slice(0, at), code: code || null, params };
}

/**
 * Whether this is a command that failed only because the silo locked.
 *
 * Locking exists to make everything stop, so an operation caught by it did
 * what it was told. The user pressed Lock a moment ago and knows; a row of
 * error toasts saying so is noise, and in-flight reads racing the lock made
 * several of them at once.
 */
export function isLockedError(err: unknown): boolean {
  return decodeAppError(err).message.toLowerCase().includes("vault is locked");
}

/** Map raw Tauri errors to short human-readable copy. */
/// The sentences written here are translated; whatever the backend sent and
/// is passed through stays as it came.
export function formatAppError(err: unknown): string {
  if (err === null || err === undefined) return t("app.err_unknown");
  const decoded = decodeAppError(err);
  // A coded error says it in the language in use. A code this build has no
  // text for falls through to the English, read like any other.
  if (decoded.code && hasKey(decoded.code)) return t(decoded.code, decoded.params);
  const msg = decoded.message;
  const lower = msg.toLowerCase();

  if (msg.includes("CloudNotConfigured") || lower.includes("no backup storage is connected")) {
    return t("app.err_not_backed_up");
  }
  // "Unlock the silo first", "enrol a key before unlocking" and friends
  // already say the right thing, so they go back unchanged. Checked before
  // the rules below, several of which would otherwise claim them. "enrol"
  // also matches the US spelling core may still send.
  if (
    (lower.includes("vaultlocked") || lower.includes("vault locked") || lower.includes("unlock")) &&
    (lower.includes("first") || lower.includes("enrol"))
  ) {
    return msg;
  }
  // OneDrive, Dropbox and Google Drive. Before the rules below: "cancelled"
  // here is the browser sign-in, and "refused access" is not a password.
  const provider = /\b(onedrive|dropbox|google drive)\b/i.exec(msg)?.[1];
  const cloudName = provider
    ? ({ onedrive: "OneDrive", dropbox: "Dropbox", "google drive": "Google Drive" } as const)[
        provider.toLowerCase() as "onedrive" | "dropbox" | "google drive"
      ]
    : null;
  if (cloudName && lower.includes(" again")) {
    if (lower.includes("sign in to")) {
      return t("app.err_cloud_sign_in_again", { cloud: cloudName });
    }
  }
  if (cloudName && lower.includes("is full")) {
    return t("app.err_cloud_full", { cloud: cloudName });
  }
  if (lower.includes("sign-in was cancelled")) {
    return t("app.err_sign_in_cancelled");
  }
  if (
    lower.includes("sign-in") ||
    lower.includes("work or school") ||
    lower.includes("daily upload limit") ||
    (cloudName && lower.includes("different"))
  ) {
    // Core's own sentences, without the storage prefix.
    const text = msg.replace(/^(storage rejected the request|storage error|could not reach storage):\s*/i, "");
    return text.charAt(0).toUpperCase() + text.slice(1);
  }
  // Cancelled and timed out are only about a key when the error came from a
  // key prompt. Mapped on the word alone, a storage timeout was reported as
  // the security key's, and a stopped upload as a cancelled key prompt.
  const fromKey =
    /security key|windows hello|touch id|webauthn|passkey|fido|ceremony|user_cancelled/.test(lower);
  const cancelled =
    lower.includes("cancelled") || lower.includes("canceled") || lower.includes("user_cancelled");
  const timedOut = lower.includes("timeout") || lower.includes("timed out");
  if (fromKey && cancelled) {
    return t("app.err_key_cancelled");
  }
  if (fromKey && timedOut) {
    return t("app.err_key_timed_out");
  }
  if (timedOut) {
    return t("app.err_storage_timed_out");
  }
  if (
    lower.includes("connection refused") ||
    lower.includes("failed to fetch") ||
    lower.includes("error sending request") ||
    lower.includes("tcp connect error")
  ) {
    return t("app.err_storage_unreachable");
  }
  // The bare numbers are matched as whole words. "401" as a substring
  // appears in file names, key ids and byte counts, and any of those turned
  // an unrelated failure into advice about storage credentials.
  if (lower.includes("unauthorized") || /\b(401|403)\b/.test(lower)) {
    return t("app.err_storage_refused");
  }
  if (lower.includes("nosuchbucket") || lower.includes("bucket does not exist")) {
    return t("app.err_no_bucket");
  }
  if (lower.includes("not enrolled") || lower.includes("no security key")) {
    return t("app.err_no_key");
  }
  if (lower.includes("already enrolled")) {
    return t("app.err_key_already_enrolled");
  }
  // Narrowed to the phrases this app writes, the current one and the older
  // "security key" wording. "at least one" alone matched sentences about
  // anything.
  if (
    lower.includes("keep at least one key") ||
    lower.includes("keep at least one security key")
  ) {
    return t("app.err_keep_one_key");
  }

  // Strip common Rust/Tauri wrappers
  const cleaned = msg
    .replace(/^error:\s*/i, "")
    .replace(/^Error:\s*/i, "")
    .replace(/^invoke\([^)]+\):\s*/i, "")
    .trim();

  return cleaned || t("app.err_generic");
}
