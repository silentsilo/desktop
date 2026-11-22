import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowUpCircle } from "lucide-react";
import { openDownloadPage } from "../lib/updater";
import { t, useLocale } from "../i18n";

type Props = {
  version: string;
  /** Hides this version for a week. */
  onPostpone: () => void;
  /** Where the download page opens. */
  open?: (url: string) => Promise<void>;
};

/**
 * The update the daily check found, on the screens before a silo opens.
 * It installs nothing: Get latest version opens the download page, and the
 * user runs the installer (docs/ARCHITECTURE.md, "Updates"). It stays until
 * that version is installed, or for a week after Postpone.
 */
export function UpdateCard({ version, onPostpone, open = openUrl }: Props) {
  useLocale();
  return (
    <section className="update-card" aria-label={t("set.update_card_label")}>
      <ArrowUpCircle size={22} className="update-card-icon" aria-hidden />
      <div className="update-card-body">
        <p className="update-card-title">{t("update.available", { version })}</p>
        <p className="update-card-text">{t("update.you_have", { current: __APP_VERSION__ })}</p>
      </div>
      <div className="update-card-actions">
        <button
          className="btn-primary"
          type="button"
          onClick={() => void openDownloadPage(open).catch(() => {})}
        >
          {t("update.get_latest")}
        </button>
        <button type="button" className="btn-secondary" onClick={onPostpone}>
          {t("update.postpone")}
        </button>
      </div>
    </section>
  );
}
