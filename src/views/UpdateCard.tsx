import { openUrl } from "@tauri-apps/plugin-opener";
import type { Update } from "@tauri-apps/plugin-updater";
import { ArrowUpCircle } from "lucide-react";
import { formatBytes } from "../lib/format";
import { useUpdater } from "./settings/AppSettings";

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
  const updater = useUpdater({ version, update }, onFailedAfterLock);
  const { state } = updater;
  const installing = state.phase === "installing";
  const progress =
    state.phase === "installing"
      ? state.contentLength
        ? `${formatBytes(state.downloaded)} of ${formatBytes(state.contentLength)}`
        : "Downloading"
      : null;

  return (
    <section className="update-card" aria-label="Update available">
      <ArrowUpCircle size={22} className="update-card-icon" aria-hidden />
      <div className="update-card-body">
        <p className="update-card-title">SilentSilo {version} is available</p>
        <p className="update-card-text">
          {progress ??
            (state.phase === "error"
              ? `It did not install: ${state.message}`
              : `You have ${__APP_VERSION__}. Installing locks any open silo and restarts SilentSilo.`)}
        </p>
        <button
          type="button"
          className="link"
          onClick={() => void open(`${RELEASES}${version}`).catch(() => {})}
        >
          What's new
        </button>
      </div>
      <div className="update-card-actions">
        <button
          type="button"
          disabled={installing}
          onClick={() => void updater.install(update, version)}
        >
          {installing ? "Installing" : "Install and restart"}
        </button>
        <button type="button" className="secondary" disabled={installing} onClick={onLater}>
          Later
        </button>
      </div>
    </section>
  );
}
