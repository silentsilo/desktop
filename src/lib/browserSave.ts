import { withEdits } from "./passwordEntry";
import type { BrowserSavePrompt, PasswordEntry } from "./types";

/** Where a new login goes when the person does not file it elsewhere. */
const SAVE_CATEGORY = "General";

/** The login a save would update, when the window has it. */
export function existingEntry(
  prompt: BrowserSavePrompt,
  entries: PasswordEntry[],
): PasswordEntry | undefined {
  return prompt.existing ? entries.find((e) => e.id === prompt.existing!.id) : undefined;
}

/** Whether the login is already in the silo exactly as the page has it. */
export function alreadySaved(prompt: BrowserSavePrompt, existing: PasswordEntry | undefined) {
  return (
    existing !== undefined &&
    existing.password === prompt.password &&
    existing.username === prompt.username
  );
}

/**
 * The entry a save from the browser writes. An update keeps everything the
 * entry had, its name included, and changes the username and password; the
 * old password goes into the history on the way to the store, as for any
 * edit. A new login is filed under the site.
 */
export function savedEntry(
  prompt: BrowserSavePrompt,
  update: PasswordEntry | undefined,
  label: string,
  username: string,
  now: number,
): { entry: PasswordEntry; updated: boolean } {
  if (update) {
    return {
      entry: withEdits(update, { username, password: prompt.password, updated_at: now }),
      updated: true,
    };
  }
  return {
    entry: {
      id: crypto.randomUUID(),
      service: label.trim() || prompt.label,
      username,
      password: prompt.password,
      url: prompt.url,
      notes: "",
      category: SAVE_CATEGORY,
      created_at: now,
      updated_at: now,
    },
    updated: false,
  };
}
