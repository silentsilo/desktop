import { useEffect, useRef, useState } from "react";
import { Settings2, X } from "lucide-react";
import type { Update } from "@tauri-apps/plugin-updater";
import type { Os } from "../../lib/platformStrings";
import { AppSettingsSection, useUpdater, type AppSectionId } from "./AppSettings";
import { t, useLocale, type Key } from "../../i18n";
import { Segmented } from "../../components/Segmented";

const TABS: { id: AppSectionId; label: Key }[] = [
  { id: "general", label: "settings.general" },
  { id: "browser", label: "settings.browser" },
  { id: "ssh", label: "settings.ssh" },
  { id: "updates", label: "settings.updates" },
];

/** How long the panel takes to slide out, matched in styles.css. */
const CLOSE_MS = 180;

type Props = {
  os: Os;
  /** Which tab to open on: Updates when an update is waiting. */
  initial: AppSectionId;
  backgroundUpdate: { version: string; update: Update } | null;
  autoUpdateEnabled: boolean;
  onAutoUpdateEnabled: (on: boolean) => void;
  defaultAutoLockMinutes: number;
  onDefaultAutoLockMinutes: (minutes: number) => void;
  onClose: () => void;
  onUpdateFailedAfterLock?: (message: string) => void;
};

/**
 * The app's settings before a silo is open, in a panel that slides in from
 * the right over the picker or the unlock screen. That screen stays where it
 * was underneath: it used to be swapped out, and a join or a restore in
 * progress there went with it.
 */
export function AppSettingsDrawer({
  os,
  initial,
  backgroundUpdate,
  autoUpdateEnabled,
  onAutoUpdateEnabled,
  defaultAutoLockMinutes,
  onDefaultAutoLockMinutes,
  onClose,
  onUpdateFailedAfterLock,
}: Props) {
  useLocale();
  const [section, setSection] = useState<AppSectionId>(initial);
  const [closing, setClosing] = useState(false);
  const updater = useUpdater(backgroundUpdate, onUpdateFailedAfterLock);
  const panel = useRef<HTMLElement>(null);
  const opener = useRef<Element | null>(null);

  const close = () => {
    if (closing) return;
    setClosing(true);
    const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    window.setTimeout(onClose, reduced ? 0 : CLOSE_MS);
  };
  const closeRef = useRef(close);
  closeRef.current = close;

  // Focus moves into the panel and back to the gear when it closes; Escape
  // closes it, as the backdrop does.
  useEffect(() => {
    opener.current = document.activeElement;
    panel.current?.focus();
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeRef.current();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      if (opener.current instanceof HTMLElement) opener.current.focus();
    };
  }, []);

  return (
    <div className={`drawer-layer${closing ? " is-closing" : ""}`}>
      <div className="drawer-backdrop" onClick={close} aria-hidden />
      <aside
        ref={panel}
        className="drawer"
        role="dialog"
        aria-modal="true"
        aria-labelledby="app-settings-title"
        tabIndex={-1}
      >
        <header className="drawer-head">
          <span className="drawer-head-icon" aria-hidden>
            <Settings2 size={18} />
          </span>
          <div className="drawer-head-text">
            <h2 id="app-settings-title">{t("set.app_title")}</h2>
            <p className="hint">{t("set.app_hint")}</p>
          </div>
          <button
            type="button"
            className="btn-ghost btn-icon"
            onClick={close}
            aria-label={t("dlg.close")}
            data-tooltip={t("dlg.close")}
          >
            <X size={18} />
          </button>
        </header>
        <div className="drawer-tabs">
          <Segmented
            kind="tab"
            label={t("set.app_title")}
            value={section}
            onChange={setSection}
            options={TABS.map((tab) => ({
              value: tab.id,
              label: t(tab.label),
              extra:
                tab.id === "updates" && updater.state.phase === "available" ? (
                  <span className="badge badge-accent">{t("settings.update_badge")}</span>
                ) : undefined,
            }))}
          />
        </div>
        <div className="drawer-body">
          <AppSettingsSection
            section={section}
            os={os}
            busy={false}
            updater={updater}
            autoUpdateEnabled={autoUpdateEnabled}
            onAutoUpdateEnabled={onAutoUpdateEnabled}
            defaultAutoLockMinutes={defaultAutoLockMinutes}
            onDefaultAutoLockMinutes={onDefaultAutoLockMinutes}
            siloHasKeys={null}
          />
        </div>
      </aside>
    </div>
  );
}
