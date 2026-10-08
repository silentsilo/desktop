import { useState } from "react";
import { ArrowLeft } from "lucide-react";
import type { Update } from "@tauri-apps/plugin-updater";
import { AuthShell } from "../../layout/AuthShell";
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

/** The app's settings, from the picker or the unlock screen. */
export function AppSettingsView({
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
  const updater = useUpdater(backgroundUpdate, onUpdateFailedAfterLock);

  return (
    <AuthShell subtitle={t("set.app_subtitle")}>
      <section className="card auth-card app-settings-card">
        <h2>{t("set.app_title")}</h2>
        <p className="hint">{t("set.app_hint")}</p>
        <div className="app-settings-tabs">
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
        <div className="auth-alt">
          <button type="button" className="link" onClick={onClose}>
            <ArrowLeft size={14} />
            {t("unlock.back")}
          </button>
        </div>
      </section>
    </AuthShell>
  );
}
