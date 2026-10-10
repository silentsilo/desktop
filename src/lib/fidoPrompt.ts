import { t, type Key } from "../i18n";

/** A live instruction from a key ceremony, as `fido-progress` sends it. */
export type FidoPrompt = { code: string; params: Record<string, string>; text: string };

const KEYS: Record<string, Key> = {
  enrol_built_in: "app.fido_enrol_built_in",
  enrol_key: "app.fido_enrol_key",
  enrol_built_in_again: "app.fido_enrol_built_in_again",
  enrol_key_again: "app.fido_enrol_key_again",
  keep_key: "app.fido_keep_key",
  finish_with_key: "app.fido_finish_with_key",
  rotating: "app.fido_rotating",
  opening: "app.fido_opening",
  rotating_count: "app.fido_rotating_count",
  touch_enrolled: "app.fido_touch_enrolled",
  unlock_built_in: "app.fido_unlock_built_in",
  unlock_key: "app.fido_unlock_key",
  unlock_any: "app.fido_unlock_any",
  verify_built_in_show_entry: "app.fido_verify_built_in_show_entry",
  verify_key_show_entry: "app.fido_verify_key_show_entry",
  verify_built_in_export: "app.fido_verify_built_in_export",
  verify_key_export: "app.fido_verify_key_export",
  verify_built_in_fill: "app.fido_verify_built_in_fill",
  verify_key_fill: "app.fido_verify_key_fill",
  verify_built_in_fill_any: "app.fido_verify_built_in_fill_any",
  verify_key_fill_any: "app.fido_verify_key_fill_any",
  verify_built_in_ssh: "app.fido_verify_built_in_ssh",
  verify_key_ssh: "app.fido_verify_key_ssh",
  verify_built_in_autotype: "app.fido_verify_built_in_autotype",
  verify_key_autotype: "app.fido_verify_key_autotype",
  org_confirm_change: "app.fido_org_confirm_change",
  org_read_log: "app.fido_org_read_log",
  org_start_log: "app.fido_org_start_log",
  org_retention: "app.fido_org_retention",
  org_prune: "app.fido_org_prune",
  org_export_log: "app.fido_org_export_log",
};

/** The instruction in the language in use; the backend's English for a
 * code this build does not know. `builtIn` names Windows Hello or Touch ID. */
export function describeFidoPrompt(prompt: FidoPrompt | string, builtIn: string): string {
  if (typeof prompt === "string") return prompt;
  const { code, params, text } = prompt;
  const name = code.startsWith("verify_") ? `${code}_${params.purpose}` : code;
  const key = KEYS[name];
  return key ? t(key, { ...params, builtIn }) : text;
}
