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
    return "No backup storage is connected. This silo is on this computer only.";
  }
  // "Unlock the silo first", "enroll a key before unlocking" and friends
  // already say the right thing, so they go back unchanged. Checked before
  // the rules below, several of which would otherwise claim them.
  if (
    (lower.includes("vaultlocked") || lower.includes("vault locked") || lower.includes("unlock")) &&
    (lower.includes("first") || lower.includes("enroll"))
  ) {
    return msg;
  }
  if (lower.includes("cancelled") || lower.includes("canceled") || lower.includes("user_cancelled")) {
    return "Security key prompt was cancelled.";
  }
  if (lower.includes("timeout") || lower.includes("timed out")) {
    return "Timed out waiting for the security key. Try again.";
  }
  if (
    lower.includes("connection refused") ||
    lower.includes("failed to fetch") ||
    lower.includes("error sending request") ||
    lower.includes("tcp connect error")
  ) {
    return "Can’t reach your storage bucket. Check your connection and the endpoint in Settings.";
  }
  // The bare numbers are matched as whole words. "401" as a substring
  // appears in file names, key ids and byte counts, and any of those turned
  // an unrelated failure into advice about storage credentials.
  if (lower.includes("unauthorized") || /\b(401|403)\b/.test(lower)) {
    return "Your storage provider rejected the access key. Check the credentials in Settings.";
  }
  if (lower.includes("nosuchbucket") || lower.includes("bucket does not exist")) {
    return "That bucket doesn’t exist. Check the name and region in Settings.";
  }
  if (lower.includes("not enrolled") || lower.includes("no security key")) {
    return "No security key enrolled yet.";
  }
  if (lower.includes("already enrolled")) {
    return "That credential is already enrolled.";
  }
  // Narrowed to the phrase this app actually writes. "at least one" alone
  // matched sentences about anything.
  if (lower.includes("keep at least one security key")) {
    return "Keep at least one security key on the silo.";
  }

  // Strip common Rust/Tauri wrappers
  const cleaned = msg
    .replace(/^error:\s*/i, "")
    .replace(/^Error:\s*/i, "")
    .replace(/^invoke\([^)]+\):\s*/i, "")
    .trim();

  return cleaned || "Something went wrong.";
}
