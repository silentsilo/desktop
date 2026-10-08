import { useEffect, useState } from "react";
import { LOCALES, languagePreference, setLanguage, systemLocale, t, useLocale } from "../../i18n";
import { CheckCircle2, DownloadCloud, ExternalLink, Globe, Info, SlidersHorizontal } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { Update } from "@tauri-apps/plugin-updater";
import { type Os, platformStrings } from "../../lib/platformStrings";
import { formatBytes } from "../../lib/format";
import { formatAppError } from "../../lib/errors";
import { SettingGroupTitle, SettingList, SettingRow, Toggle } from "../../components/Setting";
import { Segmented } from "../../components/Segmented";
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
import { HISTORY_POLICIES, type HistoryPolicy } from "../../lib/entryHistory";
import { SshAgentSettings } from "./SshAgentSettings";
import { AutoTypeSettings } from "./AutoTypeSettings";
import { loadHistoryPolicy, saveHistoryPolicy } from "../../lib/historySetting";
import { useLasting } from "../../lib/lasting";
import { setLockNotice, useLockNotice } from "../../lib/lockNotice";

/** The sections that belong to the app rather than to one silo. */
export type AppSectionId = "general" | "browser" | "ssh" | "updates";

export function formatMinutes(minutes: number): string {
  if (minutes < 60) return t("set.minutes", { count: minutes });
  return t("set.hours", { count: minutes / 60 });
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

const UPDATE_IDLE: UpdateState = { phase: "idle" };

/** The update check and install. */
export function useUpdater(
  backgroundUpdate: { version: string; update: Update } | null,
  /** Told when an install failed after locking every silo, which takes
   * this page off screen before it can show the error. */
  onFailedAfterLock?: (message: string) => void,
) {
  // One state for the whole app, kept while there is something to show:
  // the Updates page, the settings before unlock and the update card all
  // read it, and a download goes on when any of them is left.
  const [state, setState] = useLasting<UpdateState>(
    "updater",
    UPDATE_IDLE,
    (s) => s.phase === "checking" || s.phase === "installing" || s.phase === "available",
    true,
  );

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
  }, [backgroundUpdate, setState, state.phase]);

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
  useLocale();
  const platform = platformStrings(os);
  const themeControl = useTheme();
  const lockNotice = useLockNotice();
  const [historyPolicy, setHistoryPolicy] = useState<HistoryPolicy>(loadHistoryPolicy);

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

  if (section === "ssh") return <SshAgentSettings os={os} busy={busy} />;

  if (section === "general") {
    return (
      <div className="panel-section">
        <h3>
          <SlidersHorizontal size={16} />
          {t("settings.general")}
        </h3>
        <p className="lead">
          {t("set.gen_tray", { tray: platform.trayArea, fileManager: platform.fileManager })}
        </p>
        <SettingList>
          <SettingGroupTitle>{t("set.group_startup")}</SettingGroupTitle>
          <SettingRow
            label={t("set.gen_autostart", { signIn: platform.signIn })}
            htmlFor="autostart"
            hint={
              <>
                {t("set.gen_autostart_hint", { tray: platform.trayArea })} {platform.autostartHint}
              </>
            }
            extra={
              <>
                {autostart && !autostart.supported && (
                  <p className="hint">{t("set.not_available")}</p>
                )}
                {autostartError && <p className="hint is-error">{autostartError}</p>}
              </>
            }
          >
            <Toggle
              id="autostart"
              checked={autostart?.enabled ?? false}
              disabled={busy || !autostart?.supported}
              onChange={(on) => void toggleAutostart(on)}
            />
          </SettingRow>

          <SettingGroupTitle>{t("set.group_look")}</SettingGroupTitle>
          <LanguagePicker />

          {themeControl && (
            <SettingRow label={t("set.gen_theme")}>
              <Segmented<ThemeChoice>
                label={t("set.gen_theme")}
                value={themeControl.choice}
                onChange={themeControl.choose}
                options={[
                  { value: "system", label: t("set.theme_system") },
                  { value: "light", label: t("set.theme_light") },
                  { value: "dark", label: t("set.theme_dark") },
                ]}
              />
            </SettingRow>
          )}

          <SettingGroupTitle>{t("set.group_security")}</SettingGroupTitle>
          <SettingRow
            label={t("set.gen_auto_lock")}
            htmlFor="auto-lock-default"
            hint={t("set.gen_auto_lock_hint")}
          >
            <select
              id="auto-lock-default"
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
          </SettingRow>

          <SettingRow
            label={t("set.gen_lock_notice")}
            htmlFor="lock-notice"
            hint={t("set.gen_lock_notice_hint")}
          >
            <Toggle id="lock-notice" checked={lockNotice} onChange={setLockNotice} />
          </SettingRow>

          <SettingRow
            label={t("set.gen_history")}
            htmlFor="password-history"
            hint={t("set.gen_history_hint")}
          >
            <select
              id="password-history"
              value={String(historyPolicy)}
              onChange={(e) => {
                const policy: HistoryPolicy =
                  e.target.value === "fit" ? "fit" : Number.parseInt(e.target.value, 10);
                setHistoryPolicy(policy);
                saveHistoryPolicy(policy);
              }}
            >
              {HISTORY_POLICIES.map((policy) => (
                <option key={String(policy)} value={String(policy)}>
                  {policy === "fit"
                    ? t("set.gen_history_fit")
                    : t("set.gen_history_last", { count: policy })}
                </option>
              ))}
            </select>
          </SettingRow>

          <AutoTypeSettings platform={platform} />
        </SettingList>
      </div>
    );
  }

  if (section === "browser") {
    return (
      <div className="panel-section">
        <h3>
          <Globe size={16} />
          {t("settings.browser")}
        </h3>
        <p className="lead">{t("set.br_intro")}</p>
        {browserExtension?.supported && !browserExtension.bundled ? (
          <p className="hint">{t("set.br_not_bundled")}</p>
        ) : (
          <>
            <SettingList>
              <SettingRow
                label={t("set.br_allow")}
                htmlFor="browser-extension"
                hint={
                  platform.hasBuiltIn
                    ? t("set.br_allow_hint_builtin", { builtin: platform.builtIn })
                    : t("set.br_allow_hint")
                }
                extra={
                  <>
                    {browserExtension?.supported && siloHasKeys === false && (
                      <p className="hint is-error">
                        {platform.hasBuiltIn
                          ? t("set.br_no_keys_builtin", { builtin: platform.builtIn })
                          : t("set.br_no_keys")}
                      </p>
                    )}
                    {browserExtension && !browserExtension.supported && (
                      <p className="hint">{t("set.not_available_yet")}</p>
                    )}
                    {browserExtension?.enabled &&
                      !browserExtension.running &&
                      !browserExtensionError && (
                        <p className="hint is-error">{t("set.br_not_running")}</p>
                      )}
                    {browserExtensionError && (
                      <p className="hint is-error">{browserExtensionError}</p>
                    )}
                  </>
                }
              >
                <Toggle
                  id="browser-extension"
                  checked={browserExtension?.enabled ?? false}
                  disabled={busy || browserExtensionBusy || !browserExtension?.supported}
                  onChange={(on) => void toggleBrowserExtension(on)}
                />
              </SettingRow>
            </SettingList>
            {browserExtension?.enabled && (browserExtension.recent?.length ?? 0) > 0 && (
              <div className="browser-recent">
                <p className="hint">{t("set.br_recent")}</p>
                <ul>
                  {browserExtension.recent.map((fill) => (
                    <li key={`${fill.at}-${fill.site}-${fill.label}`}>
                      {t("set.br_recent_row", {
                        label: fill.label,
                        site: fill.site,
                        time: new Date(fill.at * 1000).toLocaleTimeString([], {
                          hour: "2-digit",
                          minute: "2-digit",
                        }),
                      })}
                    </li>
                  ))}
                </ul>
              </div>
            )}
            <ExtensionStoreLinks
              links={EXTENSION_STORES}
              onChosen={() => {
                if (
                  browserExtension?.supported &&
                  browserExtension.bundled &&
                  !browserExtension.enabled &&
                  !browserExtensionBusy
                ) {
                  void toggleBrowserExtension(true);
                }
              }}
            />
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
          {t("set.up_title")}
        </h3>
        <p className="lead">{t("set.up_current", { version: __APP_VERSION__ })}</p>
        {updateState.phase === "available" && (
          <div className="update-available" role="status">
            <DownloadCloud size={16} aria-hidden />
            <span>
              <strong>{t("set.up_available", { version: updateState.version })}</strong>{" "}
              {t("set.up_available_body")}
            </span>
          </div>
        )}
        {updateState.phase === "up-to-date" && (
          <p className="hint success-msg">
            <CheckCircle2 size={14} />
            {t("set.up_latest")}
          </p>
        )}
        {updateState.phase === "installing" && (
          <div className="progress-row" role="status">
            <p className="hint">
              {updateState.contentLength
                ? t("set.up_downloading_of", {
                    version: updateState.version,
                    done: formatBytes(updateState.downloaded),
                    total: formatBytes(updateState.contentLength),
                  })
                : updateState.downloaded > 0
                  ? t("set.up_downloading_so_far", {
                      version: updateState.version,
                      done: formatBytes(updateState.downloaded),
                    })
                  : t("set.up_downloading", { version: updateState.version })}
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
              className="btn-primary"
              type="button"
              onClick={() => void updater.install(updateState.update, updateState.version)}
            >
              {t("set.up_install")}
            </button>
          ) : (
            <button
              type="button"
              className="btn-secondary"
              disabled={updateState.phase === "checking" || updateState.phase === "installing"}
              onClick={() => void updater.check()}
            >
              {updateState.phase === "checking" ? t("unlock.checking") : t("set.up_check")}
            </button>
          )}
        </div>
        <SettingList separated>
          <SettingRow label={t("set.up_auto")} htmlFor="auto-update" hint={t("set.up_auto_hint")}>
            <Toggle id="auto-update" checked={autoUpdateEnabled} onChange={onAutoUpdateEnabled} />
          </SettingRow>
        </SettingList>
      </div>

      <div className="panel-section">
        <h3>
          <Info size={16} />
          {t("set.about_title")}
        </h3>
        <p className="lead">
          {platform.hasBuiltIn
            ? t("set.about_body_builtin", { version: __APP_VERSION__, builtin: platform.builtIn })
            : t("set.about_body", { version: __APP_VERSION__ })}
        </p>
        <dl className="backup-config about-meta">
          <div className="backup-config-row">
            <dt>{t("set.about_publisher")}</dt>
            <dd>Software Hive S.R.L.</dd>
          </div>
          <div className="backup-config-row">
            <dt>{t("set.about_licence")}</dt>
            <dd>{t("set.about_licence_text")}</dd>
          </div>
        </dl>
        <div className="actions">
          <button
            type="button"
            className="btn-secondary"
            onClick={() => void openUrl("https://silentsilo.com")}
          >
            <ExternalLink size={14} />
            silentsilo.com
          </button>
          <button
            type="button"
            className="btn-secondary"
            onClick={() => void openUrl("https://github.com/silentsilo/desktop")}
          >
            <ExternalLink size={14} />
            {t("set.about_source")}
          </button>
        </div>
        <p className="hint">{t("set.about_copyright", { year: new Date().getFullYear() })}</p>
      </div>
    </>
  );
}

/** Settings > General: the language, or the system's. */
function LanguagePicker() {
  useLocale();
  const [preference, setPreference] = useState(languagePreference);
  const systemName = LOCALES.find((l) => l.id === systemLocale())?.name ?? "English";
  return (
    <SettingRow label={t("settings.language")} htmlFor="language" hint={t("settings.language_hint")}>
      <select
        id="language"
        value={preference}
        onChange={(e) => {
          setPreference(e.target.value);
          setLanguage(e.target.value);
        }}
      >
        <option value="system">{t("settings.language_system", { name: systemName })}</option>
        {LOCALES.map((l) => (
          <option key={l.id} value={l.id} lang={l.id}>
            {l.reviewed ? l.name : t("settings.language_beta", { name: l.name })}
          </option>
        ))}
      </select>
    </SettingRow>
  );
}
