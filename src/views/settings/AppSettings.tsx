import { useEffect, useState } from "react";
import { CheckCircle2, DownloadCloud, ExternalLink, Globe, Info, SlidersHorizontal } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Update } from "@tauri-apps/plugin-updater";
import { platformStrings, type Os } from "../../lib/platformStrings";
import { formatBytes } from "../../lib/format";
import { formatAppError } from "../../lib/errors";
import {
  checkForUpdate,
  installUpdateAndRelaunch,
  UpdateInstallError,
} from "../../lib/updater";
import { readAutostart, writeAutostart, type AutostartStatus } from "../../lib/autostart";
import { readBrowserExtension, writeBrowserExtension } from "../../lib/browserExtension";
import { EXTENSION_STORES } from "../../lib/extensionStores";
import { useTheme, type ThemeChoice } from "../../lib/theme";
import { AUTO_LOCK_OPTIONS_MINUTES, type BrowserExtensionStatus } from "../../lib/types";
import { ExtensionStoreLinks } from "../ExtensionStoreLinks";

/** The sections that belong to the app rather than to one silo. */
export type AppSectionId = "general" | "browser" | "updates";

export function formatMinutes(minutes: number): string {
  if (minutes < 60) return `${minutes} min`;
  const hours = minutes / 60;
  return hours === 1 ? "1 hour" : `${hours} hours`;
}

export type UpdateState =
  | { phase: "idle" }
  | { phase: "checking" }
  | { phase: "up-to-date" }
  | { phase: "available"; version: string; update: Update }
  | {
      phase: "installing";
      version: string;
      /** Bytes down so far, and the whole size when the server said one. */
      downloaded: number;
      contentLength: number | null;
    }
  | { phase: "error"; message: string };

/**
 * The update check and install, held by whoever hosts the Updates page so
 * the state survives moving between sections.
 */
export function useUpdater(
  backgroundUpdate: { version: string; update: Update } | null,
  /** Told when an install failed after locking every silo, which takes
   * this page off screen before it can show the error. */
  onFailedAfterLock?: (message: string) => void,
) {
  const [state, setState] = useState<UpdateState>({ phase: "idle" });

  // An update the daily check found lands here as the initial state, so
  // opening Settings shows it without another request. Any state the user
  // has since driven (checking, installing) wins over it.
  useEffect(() => {
    if (backgroundUpdate && state.phase === "idle") {
      setState({
        phase: "available",
        version: backgroundUpdate.version,
        update: backgroundUpdate.update,
      });
    }
  }, [backgroundUpdate, state.phase]);

  const check = async () => {
    setState({ phase: "checking" });
    try {
      const result = await checkForUpdate();
      setState(
        result.available
          ? { phase: "available", version: result.version, update: result.update }
          : { phase: "up-to-date" },
      );
    } catch (e) {
      setState({ phase: "error", message: formatAppError(e) });
    }
  };

  const install = async (update: Update, version: string) => {
    setState({ phase: "installing", version, downloaded: 0, contentLength: null });
    try {
      await installUpdateAndRelaunch(update, (downloaded, contentLength) => {
        setState({ phase: "installing", version, downloaded, contentLength });
      });
    } catch (e) {
      const failure = e instanceof UpdateInstallError ? e : null;
      const message = formatAppError(failure ? failure.reason : e);
      setState({ phase: "error", message });
      if (failure?.silosLocked) onFailedAfterLock?.(message);
    }
  };

  return { state, check, install };
}

type Props = {
  section: AppSectionId;
  os: Os;
  busy: boolean;
  updater: ReturnType<typeof useUpdater>;
  autoUpdateEnabled: boolean;
  onAutoUpdateEnabled: (on: boolean) => void;
  /** The auto-lock a silo follows when it has none of its own. */
  defaultAutoLockMinutes: number;
  onDefaultAutoLockMinutes: (minutes: number) => void;
  /** Whether the open silo has a key to confirm fills with; null when no
   * silo is open, which is when this page is reached from the picker. */
  siloHasKeys: boolean | null;
};

/**
 * The app's own settings: the same for every silo, and reachable before
 * any silo is unlocked, from the picker and the unlock screen as well as
 * from Settings.
 */
export function AppSettingsSection({
  section,
  os,
  busy,
  updater,
  autoUpdateEnabled,
  onAutoUpdateEnabled,
  defaultAutoLockMinutes,
  onDefaultAutoLockMinutes,
  siloHasKeys,
}: Props) {
  const platform = platformStrings(os);
  const themeControl = useTheme();

  // Null until the first read comes back, and again if it fails: the
  // checkbox has no honest state to show before the OS has answered.
  const [autostart, setAutostart] = useState<AutostartStatus | null>(null);
  const [autostartError, setAutostartError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void readAutostart()
      .then((status) => {
        if (!cancelled) setAutostart(status);
      })
      .catch(() => {
        if (!cancelled) setAutostart(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const toggleAutostart = async (on: boolean) => {
    const previous = autostart;
    setAutostartError(null);
    setAutostart((s) => (s ? { ...s, enabled: on } : s));
    try {
      await writeAutostart(on);
    } catch (e) {
      setAutostart(previous);
      setAutostartError(formatAppError(e));
    }
  };

  // Same arrangement as autostart: null until the backend has answered.
  const [browserExtension, setBrowserExtension] = useState<BrowserExtensionStatus | null>(null);
  const [browserExtensionError, setBrowserExtensionError] = useState<string | null>(null);
  const [browserExtensionBusy, setBrowserExtensionBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    void readBrowserExtension()
      .then((status) => {
        if (!cancelled) setBrowserExtension(status);
      })
      .catch(() => {
        if (!cancelled) setBrowserExtension(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const toggleBrowserExtension = async (on: boolean) => {
    setBrowserExtensionError(null);
    setBrowserExtensionBusy(true);
    try {
      setBrowserExtension(await writeBrowserExtension(on));
    } catch (e) {
      setBrowserExtensionError(formatAppError(e));
      setBrowserExtension(await readBrowserExtension().catch(() => null));
    } finally {
      setBrowserExtensionBusy(false);
    }
  };

  const updateState = updater.state;

  if (section === "general") {
    return (
      <div className="panel-section">
        <h3>
          <SlidersHorizontal size={16} />
          General
        </h3>
        <p>
          Closing the window hides SilentSilo in {platform.trayArea}, so backup keeps running and
          the {platform.fileManager} right-click actions reach the silo you unlocked.
        </p>
        <label className="s3-checkbox">
          <input
            type="checkbox"
            checked={autostart?.enabled ?? false}
            disabled={busy || !autostart?.supported}
            onChange={(e) => void toggleAutostart(e.target.checked)}
          />
          <span>
            Start SilentSilo when I {platform.signIn}
            <span className="hint">
              It starts in {platform.trayArea} with no window and nothing unlocked. A key is still
              needed before a silo opens.
            </span>
          </span>
        </label>
        {autostart && !autostart.supported && <p className="hint">Not available on this system.</p>}
        {autostartError && <p className="hint is-error">{autostartError}</p>}
        <p className="hint">{platform.autostartHint}</p>

        <div className="settings-row">
          <label className="settings-row-label" htmlFor="auto-lock-default">
            Lock a silo after
          </label>
          <select
            id="auto-lock-default"
            className="auto-lock-select"
            value={defaultAutoLockMinutes}
            disabled={busy}
            onChange={(e) => onDefaultAutoLockMinutes(Number.parseInt(e.target.value, 10))}
          >
            {AUTO_LOCK_OPTIONS_MINUTES.map((minutes) => (
              <option key={minutes} value={minutes}>
                {formatMinutes(minutes)}
              </option>
            ))}
          </select>
        </div>
        <p className="hint">
          The default for every silo. A silo can have its own under Unlocking.
        </p>

        {themeControl && (
          <div className="settings-row">
            <label className="settings-row-label" htmlFor="theme-choice">
              Theme
            </label>
            <select
              id="theme-choice"
              className="auto-lock-select"
              value={themeControl.choice}
              onChange={(e) => themeControl.choose(e.target.value as ThemeChoice)}
            >
              <option value="system">Same as the system</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </div>
        )}
      </div>
    );
  }

  if (section === "browser") {
    return (
      <div className="panel-section">
        <h3>
          <Globe size={16} />
          Browser extension
        </h3>
        <p>
          Lets the browser extension fill logins from the silo that is open. It cannot see
          your files.
        </p>
        {browserExtension?.supported && !browserExtension.bundled ? (
          <p className="hint">The browser extension is not part of this build.</p>
        ) : (
          <>
            <label className="s3-checkbox">
              <input
                type="checkbox"
                checked={browserExtension?.enabled ?? false}
                disabled={busy || browserExtensionBusy || !browserExtension?.supported}
                onChange={(e) => void toggleBrowserExtension(e.target.checked)}
              />
              <span>
                Allow the SilentSilo browser extension
                <span className="hint">
                  You confirm every fill in this window with {platform.builtIn} or your security
                  key. Turned off, the extension cannot reach SilentSilo.
                </span>
              </span>
            </label>
            {browserExtension?.supported && siloHasKeys === false && (
              <p className="hint is-error">
                This silo has no security key or {platform.builtIn} set up, so the browser cannot
                fill anything from it. Add one under Unlocking first.
              </p>
            )}
            {browserExtension && !browserExtension.supported && (
              <p className="hint">Not available on this system yet.</p>
            )}
            {browserExtension?.enabled && !browserExtension.running && !browserExtensionError && (
              <p className="hint is-error">
                On, but the browser cannot reach SilentSilo yet. Turn it off and on again.
              </p>
            )}
            {browserExtensionError && <p className="hint is-error">{browserExtensionError}</p>}
            <ExtensionStoreLinks links={EXTENSION_STORES} />
          </>
        )}
      </div>
    );
  }

  return (
    <>
      <div className="panel-section">
        <h3>
          <DownloadCloud size={16} />
          Updates
        </h3>
        <p>Current version: v{__APP_VERSION__}</p>
        {updateState.phase === "available" && (
          <div className="update-available" role="status">
            <DownloadCloud size={16} aria-hidden />
            <span>
              <strong>Version {updateState.version} is available.</strong> Every open silo locks
              before it installs, and the app restarts on its own.
            </span>
          </div>
        )}
        {updateState.phase === "up-to-date" && (
          <p className="hint success-msg">
            <CheckCircle2 size={14} />
            You are on the latest version.
          </p>
        )}
        {updateState.phase === "installing" && (
          <div className="progress-row" role="status">
            <p className="hint">
              Downloading v{updateState.version}
              {updateState.contentLength
                ? `: ${formatBytes(updateState.downloaded)} of ${formatBytes(updateState.contentLength)}`
                : updateState.downloaded > 0
                  ? `: ${formatBytes(updateState.downloaded)} so far`
                  : ""}
              . The app restarts on its own when it is done.
            </p>
            {updateState.contentLength !== null && updateState.contentLength > 0 && (
              <div className="progress-track">
                <div
                  className="progress-fill"
                  style={{
                    width: `${Math.min(100, (updateState.downloaded / updateState.contentLength) * 100)}%`,
                  }}
                />
              </div>
            )}
          </div>
        )}
        {updateState.phase === "error" && <p className="hint is-error">{updateState.message}</p>}
        <div className="actions">
          {updateState.phase === "available" ? (
            <button
              type="button"
              onClick={() => void updater.install(updateState.update, updateState.version)}
            >
              Download and install
            </button>
          ) : (
            <button
              type="button"
              className="secondary"
              disabled={updateState.phase === "checking" || updateState.phase === "installing"}
              onClick={() => void updater.check()}
            >
              {updateState.phase === "checking" ? "Checking…" : "Check for updates"}
            </button>
          )}
        </div>
        <label className="s3-checkbox update-auto-toggle">
          <input
            type="checkbox"
            checked={autoUpdateEnabled}
            onChange={(e) => onAutoUpdateEnabled(e.target.checked)}
          />
          <span>
            Check for updates automatically
            <span className="hint">
              Once a day at most. The request sends the app version and platform, and the server
              sees your IP address. With this off, you get security fixes only when you check by
              hand.
            </span>
          </span>
        </label>
      </div>

      <div className="panel-section">
        <h3>
          <Info size={16} />
          About SilentSilo
        </h3>
        <p>
          Version {__APP_VERSION__}. An encrypted vault for files and passwords, unlocked by a
          security key or {platform.builtIn}, backed up to storage you control.
        </p>
        <dl className="backup-config">
          <div className="backup-config-row">
            <dt>Publisher</dt>
            <dd>Software Hive S.R.L.</dd>
          </div>
          <div className="backup-config-row">
            <dt>Licence</dt>
            <dd>GNU AGPL v3, provided as is, without warranty. The source code is public.</dd>
          </div>
        </dl>
        <div className="actions">
          <button
            type="button"
            className="secondary"
            onClick={() => void openUrl("https://silentsilo.com")}
          >
            <ExternalLink size={14} />
            silentsilo.com
          </button>
          <button
            type="button"
            className="secondary"
            onClick={() => void openUrl("https://github.com/silentsilo/desktop")}
          >
            <ExternalLink size={14} />
            Source code
          </button>
        </div>
        <p className="hint">
          © {new Date().getFullYear()} Software Hive S.R.L. SilentSilo is a trademark of Software
          Hive S.R.L.
        </p>
      </div>
    </>
  );
}
