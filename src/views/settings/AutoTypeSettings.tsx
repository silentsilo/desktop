import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SettingGroupTitle, SettingRow, Toggle } from "../../components/Setting";
import { builtInOrKey, type PlatformStrings } from "../../lib/platformStrings";
import { formatAppError } from "../../lib/errors";
import { t, useLocale } from "../../i18n";

type Status = {
  supported: boolean;
  enabled: boolean;
  enter: boolean;
  taken: boolean;
  /** False only on a Mac, until SilentSilo is allowed under Accessibility. */
  access: boolean;
};

/** Auto-type, under General: the shortcut on or off, and Enter after the
 * password. Rust keeps both, since it holds the shortcut before the window
 * has loaded. */
export function AutoTypeSettings({ platform }: { platform: PlatformStrings }) {
  useLocale();
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<Status>("autotype_status").then(setStatus, () => setStatus(null));
  }, []);

  if (!status) return null;

  const set = async (enabled: boolean, enter: boolean) => {
    setError(null);
    try {
      setStatus(await invoke<Status>("autotype_set", { enabled, enter }));
    } catch (e) {
      setError(formatAppError(e));
    }
  };

  return (
    <>
      <SettingGroupTitle>{t("set.group_autotype")}</SettingGroupTitle>
      <SettingRow
        label={t("set.gen_autotype", { keys: platform.autotypeKeys })}
        htmlFor="autotype"
        hint={
          <>
            {status.supported
              ? t("set.gen_autotype_hint", { method: builtInOrKey(platform) })
              : t("set.gen_autotype_unsupported")}
            {status.taken && (
              <span className="hint is-error">
                {t("set.gen_autotype_taken", { keys: platform.autotypeKeys })}
              </span>
            )}
            {status.supported && status.enabled && !status.access && (
              <span className="hint is-error">
                {t("set.gen_autotype_access")}{" "}
                <button
                  type="button"
                  className="link"
                  onClick={() => void invoke("autotype_open_access")}
                >
                  {t("set.gen_autotype_open_access")}
                </button>
              </span>
            )}
            {error && <span className="hint is-error">{error}</span>}
          </>
        }
      >
        <Toggle
          id="autotype"
          checked={status.enabled}
          disabled={!status.supported}
          onChange={(on) => void set(on, status.enter)}
        />
      </SettingRow>
      {status.supported && status.enabled && (
        <SettingRow
          label={t("set.gen_autotype_enter")}
          htmlFor="autotype-enter"
          hint={t("set.gen_autotype_enter_hint")}
        >
          <Toggle
            id="autotype-enter"
            checked={status.enter}
            onChange={(on) => void set(status.enabled, on)}
          />
        </SettingRow>
      )}
    </>
  );
}
