import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SettingRow, Toggle } from "../../components/Setting";
import { formatAppError } from "../../lib/errors";
import { t, useLocale } from "../../i18n";

type Status = {
  /** A keyring or the system's protection is there for storage secrets. */
  protected: boolean;
  /** Writing them to a file only this account can read is allowed. */
  allowed: boolean;
};

/** Under General, only on a computer with no keyring (Linux without one):
 * whether storage passwords may be kept in a file only this account can
 * read. Off until someone turns it on; saving such a password says so. */
export function UnprotectedSecretsSettings() {
  useLocale();
  const [status, setStatus] = useState<Status | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<Status>("secrets_storage_status").then(setStatus, () => setStatus(null));
  }, []);

  if (!status || status.protected) return null;

  const set = async (on: boolean) => {
    setError(null);
    try {
      setStatus(await invoke<Status>("secrets_allow_unprotected", { allow: on }));
    } catch (e) {
      setError(formatAppError(e));
    }
  };

  return (
    <SettingRow
      label={t("set.gen_secrets_file")}
      htmlFor="secrets-file"
      hint={t("set.gen_secrets_file_hint")}
      extra={error && <p className="hint is-error">{error}</p>}
    >
      <Toggle id="secrets-file" checked={status.allowed} onChange={(on) => void set(on)} />
    </SettingRow>
  );
}
