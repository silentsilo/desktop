import { useState } from "react";
import { ArrowLeft } from "lucide-react";
import type { Update } from "@tauri-apps/plugin-updater";
import { AuthShell } from "../../layout/AuthShell";
import type { Os } from "../../lib/platformStrings";
import { AppSettingsSection, useUpdater, type AppSectionId } from "./AppSettings";

const TABS: { id: AppSectionId; label: string }[] = [
  { id: "general", label: "General" },
  { id: "browser", label: "Browser extension" },
  { id: "updates", label: "Updates and about" },
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
  const [section, setSection] = useState<AppSectionId>(initial);
  const updater = useUpdater(backgroundUpdate, onUpdateFailedAfterLock);

  return (
    <AuthShell subtitle="App settings, the same for every silo">
      <section className="card auth-card app-settings-card">
        <h2>App settings</h2>
        <p className="hint">The same for every silo on this computer.</p>
        <div className="app-settings-tabs" role="tablist">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              type="button"
              role="tab"
              aria-selected={section === tab.id}
              className={section === tab.id ? "" : "secondary"}
              onClick={() => setSection(tab.id)}
            >
              {tab.label}
              {tab.id === "updates" && updater.state.phase === "available" && (
                <span className="tab-badge tab-badge-update rail-update-badge">New</span>
              )}
            </button>
          ))}
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
            Back
          </button>
        </div>
      </section>
    </AuthShell>
  );
}
