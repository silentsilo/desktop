import { openUrl } from "@tauri-apps/plugin-opener";
import type { Update } from "@tauri-apps/plugin-updater";
import { ArrowUpCircle } from "lucide-react";
import { formatBytes } from "../lib/format";
import { useUpdater } from "./settings/AppSettings";
import { t, useLocale } from "../i18n";

type Props = {
  version: string;
  update: Update;
  /** Hides the card until the next start. */
  onLater: () => void;
  onFailedAfterLock: (message: string) => void;
  /** Where the release notes are read; the manifest carries none. */
  open?: (url: string) => Promise<void>;
};

const RELEASES = "https://github.com/silentsilo/desktop/releases/tag/v";

/**
 * The update the daily check found, on the screens before a silo opens: no
 * silo is open there, so the restart loses nothing. A toast said the same
 * once per version and was mostly closed unread; this stays until it is
 * installed, or until the next start after Later.
 */
export function UpdateCard({ version, update, onLater, onFailedAfterLock, open = openUrl }: Props) {
  useLocale();
  const updater = useUpdater({ version, update }, onFailedAfterLock);
  const { state } = updater;
  const installing = state.phase === "installing";
  const progress =
    state.phase === "installing"
      ? state.contentLength
        ? t("update.progress", {
            done: formatBytes(state.downloaded),
            total: formatBytes(state.contentLength),
          })
        : t("update.downloading")
      : null;

  return (
    <section className="update-card" aria-label={t("set.update_card_label")}>
      <ArrowUpCircle size={22} className="update-card-icon" aria-hidden />
      <div className="update-card-body">
        <p className="update-card-title">{t("update.available", { version })}</p>
        <p className="update-card-text">
          {progress ??
            (state.phase === "error"
              ? t("update.failed", { reason: state.message })
              : t("update.you_have", { current: __APP_VERSION__ }))}
        </p>
        <button
          type="button"
          className="link"
          onClick={() => void open(`${RELEASES}${version}`).catch(() => {})}
        >
          {t("update.whats_new")}
        </button>
      </div>
      <div className="update-card-actions">
        <button
          type="button"
          disabled={installing}
          onClick={() => void updater.install(update, version)}
        >
          {installing ? t("update.installing") : t("update.install")}
        </button>
        <button type="button" className="secondary" disabled={installing} onClick={onLater}>
          {t("update.later")}
        </button>
      </div>
    </section>
  );
}
