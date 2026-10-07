/**
 * The source catalog. Every text the app shows, in English, with a note on
 * where it appears and what it means: translations are made with the note
 * and the screen in front, never word for word. A word with several senses
 * in the app is spelt out in the note (see `planuri/lansare-1.5.md`, i18n:
 * "Copy" the action or a backup copy, "Key" a security key or an encryption
 * key, "Lock" the verb or the padlock).
 *
 * `{name}` marks a value filled in; an entry with plural forms is chosen by
 * its `count`. Keys are named by where the text appears.
 */
export type Plural = { one: string; other: string };
export type Entry = { text: string | Plural; note: string };

export const en = {
  // Sidebar
  "nav.favorites": { text: "Favourites", note: "Sidebar item: files and entries the user starred." },
  "nav.files": { text: "Files", note: "Sidebar item: the file explorer of the open silo." },
  "nav.passwords": { text: "Passwords", note: "Sidebar item: logins, cards, notes and SSH keys." },
  "nav.health": {
    text: "Health",
    note: "Sidebar item: weak, reused or old passwords and backup problems, as in a health check.",
  },
  "nav.trash": { text: "Trash", note: "Sidebar item: deleted files waiting to be removed for good." },
  "nav.activity": {
    text: "Activity",
    note: "Sidebar item: the log of what happened in the silo (unlocks, copied passwords).",
  },
  "nav.settings": { text: "Settings", note: "Sidebar item: the silo's and the app's settings." },
  "nav.update_available": {
    text: "{item} (version {version} is available)",
    note: "Tooltip on a sidebar item when an app update is waiting. {item} is the item's name.",
  },
  "nav.to_look_at": {
    text: "{item} ({count} to look at)",
    note: "Tooltip on Health with a number of findings. {item} is the item's name.",
  },
  "nav.update_badge": {
    text: "Update",
    note: "Small badge on Settings in the sidebar: an app update is waiting. Noun.",
  },
  "nav.lock": {
    text: "Lock",
    note: "Button, verb: closes the open silo so a key is needed again. Not the padlock.",
  },
  "nav.lock_silo": { text: "Lock silo", note: "Tooltip on the Lock button. Verb." },
  "nav.switch_silo": {
    text: "{name}. Click to switch silo.",
    note: "Tooltip on the silo name at the top of the sidebar. A silo is the user's encrypted vault.",
  },
  "nav.expand": { text: "Expand sidebar", note: "Tooltip: makes the sidebar wide again." },
  "nav.collapse": { text: "Collapse sidebar", note: "Tooltip: makes the sidebar narrow, icons only." },
  "nav.dark": { text: "Switch to dark mode", note: "Tooltip on the theme button." },
  "nav.light": { text: "Switch to light mode", note: "Tooltip on the theme button." },

  // Settings > General: language
  "settings.language": { text: "Language", note: "Label of the language picker in Settings > General." },
  "settings.language_system": {
    text: "Same as the system ({name})",
    note: "First choice in the language picker: follow the operating system. {name} is the language that gives.",
  },
  "settings.language_beta": {
    text: "{name} (beta)",
    note: "A language in the picker whose translation a native speaker has not read yet.",
  },
  "settings.language_hint": {
    text: "Languages marked beta have not been read by a native speaker yet. Recovery codes and keys work the same in every language.",
    note: "Hint under the language picker.",
  },

  // The update card on the screens before a silo opens
  "update.available": {
    text: "SilentSilo {version} is available",
    note: "Title of the card shown when an app update is ready to install. SilentSilo is the app's name, not translated.",
  },
  "update.you_have": {
    text: "You have {current}. Installing locks any open silo and restarts SilentSilo.",
    note: "Under the title. {current} is the version installed now. 'Locks' is the verb: open silos are closed.",
  },
  "update.whats_new": { text: "What's new", note: "Link to the release notes on the web." },
  "update.install": {
    text: "Install and restart",
    note: "Button: downloads the update, installs it and starts the app again.",
  },
  "update.installing": { text: "Installing", note: "The same button while the update installs." },
  "update.later": {
    text: "Later",
    note: "Button: hides the card until the app is started again. Does not skip the version.",
  },
  "update.failed": {
    text: "It did not install: {reason}",
    note: "Shown when the update failed. {reason} comes from the system, maybe in English.",
  },
  "update.progress": {
    text: "{done} of {total}",
    note: "Download progress, sizes like '3 MB of 12 MB'.",
  },
  "update.downloading": { text: "Downloading", note: "While the update downloads, size unknown." },
  "unlock.subtitle_code": {
    text: "Unlock with the code you wrote down.",
    note: "Under the logo when unlocking with the recovery code instead of a key.",
  },
  "unlock.subtitle_rebuild": {
    text: "Enter the code you wrote down to rebuild this silo from backup storage.",
    note: "Under the logo when this computer's copy is damaged and is rebuilt from the backup copies.",
  },
  "unlock.recovery_code_title": {
    text: "Recovery code",
    note: "Card title. The code on paper that opens the silo when every key is lost.",
  },
  "unlock.recovery_code_hint": {
    text: "The code you saved when you set this up. Paste the whole code into any box and the rest fill in.",
    note: "Hint above the code boxes. 'Boxes': the separate input fields of the code.",
  },
  "unlock.code_label": {
    text: "Code",
    note: "Label of the recovery code input.",
  },
  "unlock.checking": {
    text: "Checking…",
    note: "Button text while the code or the key is being checked.",
  },
  "unlock.rebuild_and_unlock": {
    text: "Rebuild and unlock",
    note: "Button: rebuilds the damaged local copy from backup, then opens the silo.",
  },
  "unlock.unlock": {
    text: "Unlock",
    note: "Main button, verb: opens the silo.",
  },
  "unlock.back": {
    text: "Back",
    note: "Button: returns to the previous screen.",
  },
  "unlock.subtitle_builtin": {
    text: "Confirm with {builtin} to unlock.",
    note: "Under the logo when the only way in is the computer's own sign-in. {builtin} is Windows Hello or Touch ID.",
  },
  "unlock.subtitle_both": {
    text: "Touch an enrolled security key, or confirm with {builtin}, to unlock.",
    note: "Under the logo when both a security key and Windows Hello/Touch ID work. 'Touch': the gold disc on a YubiKey.",
  },
  "unlock.subtitle_key": {
    text: "Insert an enrolled security key and touch it to unlock.",
    note: "Under the logo when only a security key (hardware, like a YubiKey) opens the silo.",
  },
  "unlock.os_prompt": {
    text: "{os} will show its own prompt.",
    note: "Hint: the operating system ({os}: Windows, Linux, macOS) opens its own window asking for the key.",
  },
  "unlock.title_fallback": {
    text: "Unlock silo",
    note: "Card title when the silo has no name. Verb.",
  },
  "unlock.retry": {
    text: "Retry detection",
    note: "Button: looks again for a security key or Windows Hello.",
  },
  "unlock.waiting": {
    text: "Waiting…",
    note: "Button text while waiting for the key to be touched.",
  },
  "unlock.builtin_not_working": {
    text: "{builtin} not working? Use your recovery code",
    note: "Link under the button when Windows Hello/Touch ID is the only way in.",
  },
  "unlock.lost_key": {
    text: "Lost your key? Use your recovery code",
    note: "Link under the button. 'Key': the security key (hardware).",
  },
  "unlock.switch_silo": {
    text: "Switch silo",
    note: "Link: back to the list of silos, to open another one.",
  },
} satisfies Record<string, Entry>;

export type Key = keyof typeof en;
