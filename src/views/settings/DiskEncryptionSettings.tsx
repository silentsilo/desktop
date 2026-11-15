import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { SettingRow } from "../../components/Setting";
import { t, useLocale } from "../../i18n";
import { DISK_ENCRYPTION_HELP, type DiskEncryption } from "../../lib/diskEncryption";

/** Under General, shown only when the disk opened files land on is not
 * encrypted. An unknown answer shows nothing: a wrong warning is worse. */
export function DiskEncryptionSettings() {
  useLocale();
  const [status, setStatus] = useState<DiskEncryption | null>(null);

  useEffect(() => {
    invoke<DiskEncryption>("disk_encryption_status").then(setStatus, () => setStatus(null));
  }, []);

  if (!status || status.encrypted !== false) return null;

  const hint =
    status.os === "windows"
      ? t("set.gen_disk_hint_windows")
      : status.os === "macos"
        ? t("set.gen_disk_hint_macos")
        : t("set.gen_disk_hint_linux");
  const help = DISK_ENCRYPTION_HELP[status.os];

  return (
    <SettingRow label={t("set.gen_disk")} hint={hint}>
      {help && (
        <button type="button" className="btn-secondary" onClick={() => void openUrl(help)}>
          {t("set.gen_disk_how")}
        </button>
      )}
    </SettingRow>
  );
}
