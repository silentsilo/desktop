import type { Bootstrap, Os } from "./types";

export type { Os };

/**
 * The words that change with the operating system, in one place.
 *
 * The screens used to say "Windows Hello" and "Explorer" outright, which was
 * true for as long as Windows was the only build. A Mac has Touch ID and
 * Finder, and telling its owner to "confirm with Windows Hello" sends them
 * looking for a prompt that will never come. Everything platform-shaped is
 * an atom here; the sentences around the atoms stay in the views, so the
 * Windows wording is exactly what it was.
 */
export type PlatformStrings = {
  os: Os;
  /** "Windows" or "macOS": the name that starts "… will show its own prompt". */
  osName: string;
  /** The built-in authenticator: "Windows Hello" or "Touch ID". */
  builtIn: string;
  /** Where files live on this system: "Windows Explorer" or "Finder". */
  fileManager: string;
  /** Completes "Start SilentSilo when I …". */
  signIn: string;
  /** Shown when the built-in authenticator is not set up, with the fix. */
  builtInSetupHint: string;
  /** Where the OS shows the autostart entry, so turning it off there makes sense. */
  autostartHint: string;
  /** The floor this build needs for security keys to work at all. */
  fidoUnavailable: string;
  /** Windows offers a phone over a QR code during a key ceremony; nothing
   * else does, so the sentence explaining that only belongs there. */
  offersPhone: boolean;
};

const WINDOWS: PlatformStrings = {
  os: "windows",
  osName: "Windows",
  builtIn: "Windows Hello",
  fileManager: "Windows Explorer",
  signIn: "sign in to Windows",
  builtInSetupHint:
    "Windows Hello is not set up on this machine, so a security key is the only way in " +
    "here. Add a PIN or a fingerprint in Windows sign-in settings and Hello shows up as " +
    "a second option.",
  autostartHint:
    "Windows lists this under Startup apps in Task Manager. Turning it off there and " +
    "turning it off here are the same thing.",
  fidoUnavailable:
    "FIDO2 is not available on this system. Use Windows 10 (1903+) or later with a " +
    "compatible security key.",
  offersPhone: true,
};

const MACOS: PlatformStrings = {
  os: "macos",
  osName: "macOS",
  builtIn: "Touch ID",
  fileManager: "Finder",
  signIn: "log in to this Mac",
  builtInSetupHint:
    "Touch ID is not set up on this Mac, so a security key is the only way in here. " +
    "Add a fingerprint under Touch ID & Password in System Settings and Touch ID shows " +
    "up as a second option.",
  autostartHint:
    "macOS lists this under Login Items in System Settings. Turning it off there and " +
    "turning it off here are the same thing.",
  fidoUnavailable:
    "FIDO2 is not available on this system. Use macOS 13 or later with a compatible " +
    "security key.",
  offersPhone: false,
};

/** Not a shipping platform. Named honestly rather than pretending to be one. */
const LINUX: PlatformStrings = {
  os: "linux",
  osName: "Linux",
  builtIn: "the built-in authenticator",
  fileManager: "the file manager",
  signIn: "log in",
  builtInSetupHint: "This build has no built-in authenticator, so a security key is the only way in.",
  autostartHint: "Turning autostart off in the desktop's own settings and here are the same thing.",
  fidoUnavailable: "FIDO2 is not available on this system. Use a compatible security key.",
  offersPhone: false,
};

export function platformStrings(os: Os): PlatformStrings {
  switch (os) {
    case "macos":
      return MACOS;
    case "linux":
      return LINUX;
    default:
      return WINDOWS;
  }
}

/**
 * The platform the backend reports, defaulting to Windows.
 *
 * The field is additive: a backend that predates it, or a mock that leaves it
 * out, means the only platform that existed before it, which is the one
 * every string used to assume anyway.
 */
export function osOf(bootstrap: Pick<Bootstrap, "os"> | null | undefined): Os {
  return bootstrap?.os ?? "windows";
}
