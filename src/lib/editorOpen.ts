/** Whether the password editor is open, for navigation that would unmount
 * it. Leaving the Passwords view dropped the draft without a word, and a
 * password just generated, copied and set on a site was then stored
 * nowhere. */
export const passwordEditorOpen = { current: false };
