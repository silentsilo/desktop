/** Map raw Tauri errors to short human-readable copy. */
export function formatAppError(err: unknown): string {
  const msg = String(err ?? "Unknown error");
  const lower = msg.toLowerCase();

  if (msg.includes("CloudNotConfigured") || lower.includes("no backup storage is connected")) {
    return "No backup storage is connected. This silo is on this computer only.";
  }
  if (lower.includes("vaultlocked") || lower.includes("vault locked") || lower.includes("unlock")) {
    if (lower.includes("first") || lower.includes("enroll")) {
      return msg;
    }
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
  if (lower.includes("unauthorized") || lower.includes("401") || lower.includes("403")) {
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
  if (lower.includes("keep at least one") || lower.includes("at least one")) {
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
