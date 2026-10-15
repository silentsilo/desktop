import type { Bootstrap } from "./types";

const NEW_ISSUE = "https://github.com/silentsilo/desktop/issues/new";

/**
 * A GitHub issue filled in with what this computer offers for unlocking,
 * for a setup that did not work. Only what the app already knows about the
 * platform goes in: no silo name, no path, no key, nothing that names the
 * person or the machine. The browser shows it before anything is sent.
 */
export function setupReportUrl(
  bootstrap: Pick<
    Bootstrap,
    | "os"
    | "fido_available"
    | "fido_key_present"
    | "platform_authenticator"
    | "platform_status"
    | "phone_available"
  >,
  version: string,
): string {
  const body = [
    "What happened (which way in you chose, and what you saw):",
    "",
    "",
    "---",
    `App: ${version}`,
    `System: ${bootstrap.os ?? "windows"}`,
    `Security keys: ${bootstrap.fido_available ? "available" : "unavailable"}${
      bootstrap.fido_key_present ? ", one plugged in" : ""
    }`,
    `Built-in (Windows Hello or Touch ID): ${bootstrap.platform_status ?? "unknown"}`,
    `Phone over QR code: ${bootstrap.phone_available ? "available" : "unavailable"}`,
  ].join("\n");
  const params = new URLSearchParams({ title: "Setup did not work", body });
  return `${NEW_ISSUE}?${params.toString()}`;
}
