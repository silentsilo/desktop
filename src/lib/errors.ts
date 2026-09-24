/**
 * Whether this is a command that failed only because the silo locked.
 *
 * Locking exists to make everything stop, so an operation caught by it did
 * what it was told. The user pressed Lock a moment ago and knows; a row of
 * error toasts saying so is noise, and in-flight reads racing the lock made
 * several of them at once.
 */
export function isLockedError(err: unknown): boolean {
  return String(err ?? "")
    .toLowerCase()
    .includes("vault is locked");
}

/** Map raw Tauri errors to short human-readable copy. */
export function formatAppError(err: unknown): string {
  const msg = String(err ?? "Unknown error");
  const lower = msg.toLowerCase();

  if (msg.includes("CloudNotConfigured") || lower.includes("no backup storage is connected")) {
    return "Not backed up. This silo is only on this computer.";
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
  // Cancelled and timed out are only about a key when the error came from a
  // key prompt. Mapped on the word alone, a storage timeout was reported as
  // the security key's, and a stopped upload as a cancelled key prompt.
  const fromKey =
    /security key|windows hello|touch id|webauthn|passkey|fido|ceremony|user_cancelled/.test(lower);
  const cancelled =
    lower.includes("cancelled") || lower.includes("canceled") || lower.includes("user_cancelled");
  const timedOut = lower.includes("timeout") || lower.includes("timed out");
  if (fromKey && cancelled) {
    return "The key prompt was cancelled.";
  }
  if (fromKey && timedOut) {
    return "The key prompt timed out. Try again.";
  }
  if (timedOut) {
    return "Your backup storage did not answer in time. Check your connection and try again.";
  }
  if (
    lower.includes("connection refused") ||
    lower.includes("failed to fetch") ||
    lower.includes("error sending request") ||
    lower.includes("tcp connect error")
  ) {
    return "Cannot reach your backup storage. Check your connection and the address.";
  }
  // The bare numbers are matched as whole words. "401" as a substring
  // appears in file names, key ids and byte counts, and any of those turned
  // an unrelated failure into advice about storage credentials.
  if (lower.includes("unauthorized") || /\b(401|403)\b/.test(lower)) {
    return "Your backup storage refused the sign-in. Check the username and password, or the access key.";
  }
  if (lower.includes("nosuchbucket") || lower.includes("bucket does not exist")) {
    return "That bucket does not exist. Check its name and region.";
  }
  if (lower.includes("not enrolled") || lower.includes("no security key")) {
    return "No key enrolled yet.";
  }
  if (lower.includes("already enrolled")) {
    return "That key is already enrolled.";
  }
  // Narrowed to the phrases this app writes, the current one and the older
  // "security key" wording. "at least one" alone matched sentences about
  // anything.
  if (
    lower.includes("keep at least one key") ||
    lower.includes("keep at least one security key")
  ) {
    return "Keep at least one key on the silo.";
  }

  // Strip common Rust/Tauri wrappers
  const cleaned = msg
    .replace(/^error:\s*/i, "")
    .replace(/^Error:\s*/i, "")
    .replace(/^invoke\([^)]+\):\s*/i, "")
    .trim();

  return cleaned || "Something went wrong.";
}
