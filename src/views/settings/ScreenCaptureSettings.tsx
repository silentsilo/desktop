import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SettingRow, Toggle } from "../../components/Setting";
import { formatAppError } from "../../lib/errors";
import { t, useLocale } from "../../i18n";

type Status = {
  /** Windows and macOS can keep a window out of captures; Linux cannot. */
  supported: boolean;
  protected: boolean;
};

/** Under General: the window out of screenshots, recordings and screen
 * sharing, on unless turned off. Rust keeps the choice and applies it before
 * the window first shows. */
export function ScreenCaptureSettings() {
  useLocale();
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<Status>("screen_capture_status").then(setStatus, () => setStatus(null));
  }, []);

  if (!status) return null;

  const set = async (on: boolean) => {
    setError(null);
    try {
      setStatus(await invoke<Status>("screen_capture_set", { protected: on }));
    } catch (e) {
      setError(formatAppError(e));
    }
  };

  return (
    <SettingRow
      label={t("set.gen_capture")}
      htmlFor="screen-capture"
      hint={status.supported ? t("set.gen_capture_hint") : t("set.gen_capture_unsupported")}
      extra={error && <p className="hint is-error">{error}</p>}
    >
      <Toggle
        id="screen-capture"
        checked={status.protected}
        disabled={!status.supported}
        onChange={(on) => void set(on)}
      />
    </SettingRow>
  );
}
