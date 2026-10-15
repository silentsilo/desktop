import { t } from "../i18n";
import type { Bootstrap, Os, PlatformStatus } from "./types";

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
  /** Whether there is one the app uses. Linux has none: a security key only. */
  hasBuiltIn: boolean;
  /** Where files live on this system: "Windows Explorer" or "Finder". */
  fileManager: string;
  /** Where the app sits while its window is closed. */
  trayArea: string;
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
  /** The auto-type shortcut as this system writes it: "Ctrl+Alt+A" (German
   * "Strg+Alt+A"), or "⌃⌥A" on a Mac. */
  autotypeKeys: string;
};

// The generic words are getters, so each read is in the language in use.
// Product names (Windows Hello, Touch ID, Finder) stay as they are.
const WINDOWS: PlatformStrings = {
  os: "windows",
  osName: "Windows",
  builtIn: "Windows Hello",
  hasBuiltIn: true,
  fileManager: "Windows Explorer",
  get trayArea() {
    return t("app.platform_tray_windows");
  },
  get signIn() {
    return t("app.platform_sign_in_windows");
  },
  get builtInSetupHint() {
    return t("app.platform_setup_hint_windows");
  },
  get autostartHint() {
    return t("app.platform_autostart_windows");
  },
  get fidoUnavailable() {
    return t("app.platform_fido_unavailable_windows");
  },
  offersPhone: true,
  get autotypeKeys() {
    return t("app.platform_autotype_keys");
  },
};

const MACOS: PlatformStrings = {
  os: "macos",
  osName: "macOS",
  builtIn: "Touch ID",
  hasBuiltIn: true,
  fileManager: "Finder",
  get trayArea() {
    return t("app.platform_tray_macos");
  },
  get signIn() {
    return t("app.platform_sign_in_macos");
  },
  get builtInSetupHint() {
    return t("app.platform_setup_hint_macos");
  },
  get autostartHint() {
    return t("app.platform_autostart_macos");
  },
  get fidoUnavailable() {
    return t("app.platform_fido_unavailable_macos");
  },
  offersPhone: false,
  autotypeKeys: "⌃⌥A",
};

/** No built-in authenticator the app uses: a security key opens a silo. */
const LINUX: PlatformStrings = {
  os: "linux",
  osName: "Linux",
  get builtIn() {
    return t("app.platform_builtin_linux");
  },
  hasBuiltIn: false,
  get fileManager() {
    return t("app.platform_file_manager_linux");
  },
  get trayArea() {
    return t("app.platform_tray_linux");
  },
  get signIn() {
    return t("app.platform_sign_in_linux");
  },
  get builtInSetupHint() {
    return t("app.platform_setup_hint_linux");
  },
  get autostartHint() {
    return t("app.platform_autostart_linux");
  },
  get fidoUnavailable() {
    return t("app.platform_fido_unavailable_linux");
  },
  offersPhone: false,
  get autotypeKeys() {
    return t("app.platform_autotype_keys");
  },
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

/** What to do about the built-in authenticator, by the reason the backend
 * gave. A backend that gives none gets the platform's general hint. */
export function builtInHint(
  platform: PlatformStrings,
  status: PlatformStatus | undefined,
  phone = false,
): string {
  switch (status) {
    // "A security key is the only way in" is not true where a phone works.
    case "no_tpm":
      return phone ? t("app.platform_hint_no_tpm_phone") : t("app.platform_hint_no_tpm");
    case "update_windows":
      return phone
        ? t("app.platform_hint_update_windows_phone")
        : t("app.platform_hint_update_windows");
    case "unavailable":
      return t("app.platform_hint_touchid_unavailable");
    case "absent":
      if (platform.os === "macos") return t("app.platform_hint_no_touchid");
      return phone && platform.os === "windows"
        ? t("app.platform_setup_hint_windows_phone")
        : platform.builtInSetupHint;
    default:
      return phone && platform.os === "windows"
        ? t("app.platform_setup_hint_windows_phone")
        : platform.builtInSetupHint;
  }
}

/** Why security keys cannot be used here. On Linux a key that is plugged in
 * but not readable by this account is its own case, with its own fix. */
export function keysUnavailable(
  platform: PlatformStrings,
  bootstrap: Pick<Bootstrap, "fido_key_present">,
): string {
  if (platform.os === "linux" && bootstrap.fido_key_present) {
    return t("app.platform_fido_no_access_linux");
  }
  return platform.fidoUnavailable;
}

/** What answers a confirmation: any key on the silo, whichever kinds it
 * has (a security key, a phone, Windows Hello or Touch ID). */
export function builtInOrKey(_platform: PlatformStrings): string {
  return t("app.platform_any_key");
}

/** What happens once the key is asked for. Windows shows a dialog of its
 * own; on a Mac and on Linux the app talks to a security key itself and
 * asks for its PIN in its own window. */
export function ownPromptHint(platform: PlatformStrings): string {
  switch (platform.os) {
    case "macos":
      return t("app.own_prompt_macos");
    case "linux":
      return t("app.own_prompt_linux");
    default:
      return t("unlock.os_prompt", { os: platform.osName });
  }
}
