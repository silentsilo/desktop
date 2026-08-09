import type { SecurityKeyInfo } from "./types";

/**
 * The one kind of key this build can unlock with.
 *
 * Mirrors `KIND_FIDO2` in `silentsilo-vault`. A silo synced between machines
 * carries every device's keys, so a Windows build can be shown a key enrolled
 * by a Mac. It is a real key, it just derives its wrapping key from something
 * this build cannot ask for.
 */
export const KIND_FIDO2 = "fido2";

/** Whether a ceremony on this machine could end with the silo open. */
export function usableHere(key: SecurityKeyInfo): boolean {
  return (key.kind ?? KIND_FIDO2) === KIND_FIDO2;
}

/**
 * One name per key, everywhere.
 *
 * An unnamed key used to be "Slot 2" in the key list, "slot 2" again in the
 * same row's detail, and "Security key" in the rotation checklist, which
 * reads as three different keys. The fallback names what the key actually
 * is: the built-in authenticator is Windows Hello, and a removable one is
 * named by the slot it occupies.
 *
 * "Built-in" is only Windows Hello when the key is one this build enrolled.
 * The same flag on a key from another machine means that machine's built-in
 * authenticator, so naming it Hello would put a Windows name on a MacBook's
 * fingerprint reader.
 */
export function securityKeyDisplayName(key: SecurityKeyInfo): string {
  if (key.label) return key.label;
  if (!usableHere(key)) return "Key from another device";
  return key.platform ? "Windows Hello" : `Key in slot ${key.key_slot}`;
}
