import { useState, type ReactNode } from "react";
import { platformStrings, type Os } from "../lib/platformStrings";
import {
  AlertTriangle,
  Building2,
  CheckCircle2,
  CloudUpload,
  Copy,
  DownloadCloud,
  Globe,
  Inbox,
  KeyRound,
  LayoutDashboard,
  Laptop,
  LifeBuoy,
  SearchCheck,
  SlidersHorizontal,
  Timer,
  Wrench,
} from "lucide-react";
import { FolderHeart, Settings2 } from "lucide-react";
import { RotateKeyPanel } from "./RotateKeyPanel";
import { EmergencyKitPanel } from "./EmergencyKitPanel";
import { ViewHeader } from "../components/ViewHeader";
import type {
  Authenticator,
  DeviceInfo,
  RecoveryStatus,
  SecurityKeyInfo,
  Silo,
} from "../lib/types";
import { formatDate, formatDay } from "../lib/format";
import { securityKeyDisplayName, usableHere } from "../lib/keyName";
import { AUTO_LOCK_OPTIONS_MINUTES } from "../lib/types";
import { ActivityList } from "./ActivityList";
import { ProtectedFoldersPanel } from "./ProtectedFolders";
import type { Update } from "@tauri-apps/plugin-updater";
import type { SyncIndicator } from "../layout/AppShell";
import { AppSettingsSection, formatMinutes, useUpdater } from "./settings/AppSettings";
import { OverviewPanel } from "./settings/OverviewPanel";

type Props = {
  /** Which platform's words to use for the built-in authenticator and the shell. */
  os: Os;
  busy: boolean;
  /** Whether the daily scheduled update check is on. */
  autoUpdateEnabled: boolean;
  onAutoUpdateEnabled: (on: boolean) => void;
  /** An update the scheduled check already found, so this panel can offer
   * the install without a second request. */
  backgroundUpdate: { version: string; update: Update } | null;
  /** An install that failed after locking every silo, which closes this
   * panel before it can show the error. */
  onUpdateFailedAfterLock: (message: string) => void;
  securityKeys: SecurityKeyInfo[];
  newKeyLabel: string;
  onNewKeyLabel: (v: string) => void;
  fidoProgress: string | null;
  keyAddSuccess: string | null;
  onAddKey: (authenticator: Authenticator, organisation: boolean) => void;
  onRemoveKey: (credentialId: string) => void;
  /** Changes the key the whole silo is encrypted under. */
  onRotateKey: (keep: string[]) => void;
  /** A key change that was started and never finished. */
  rotationPending: boolean;
  onResumeRotation: (credential: string) => void;
  onRenameKey: (credentialId: string, label: string) => void;
  /** Every device that has written to this silo's log. */
  devices: DeviceInfo[];
  onRenameDevice: (deviceId: string, label: string) => void;
  /** Whether this machine's built-in authenticator can be enrolled. */
  platformAvailable: boolean;
  silo: Silo;
  onRenameSilo: (name: string) => void;
  onForgetSilo: () => void;
  onSwitchSilo: () => void;
  recovery: RecoveryStatus;
  /** Non-null only while a freshly generated code is on screen. */
  recoveryCode: string | null;
  onGenerateRecovery: () => void;
  onDisableRecovery: () => void;
  onCopyRecoveryCode: () => void;
  onDismissRecoveryCode: () => void;
  /** The fallback a silo follows when it has no timeout of its own. */
  autoLockMinutes: number;
  onAutoLockMinutes: (minutes: number) => void;
  /** This silo's own timeout, or null to follow the default. */
  siloAutoLockMinutes: number | null;
  onSiloAutoLockMinutes: (minutes: number | null) => void;
  /** The backup status, for the overview. */
  sync: SyncIndicator;
  /** Never-delete copies, which a key replacement cannot reach. */
  archiveTargets: number;
  /** Whether this computer holds every file, so it counts as a copy. */
  fullCopy: boolean;
  /** Which section is open. Held by the caller so the sidebar's storage
   * figure can open Settings straight at Backup. */
  section: SettingsSectionId;
  onSection: (id: SettingsSectionId) => void;
  /** The backup page, with the list of copies in it, hosted here rather
   * than configured here: what it needs comes from the app shell. */
  backupPanel: ReactNode;
  /** The backup test page, same arrangement as the backup page. */
  verifyPanel: ReactNode;
};

/**
 * The sections, in rail order: this silo first, then the app. The app's
 * sections are the same for every silo and also open from the picker.
 */
const SECTIONS = [
  { id: "overview", group: "silo", label: "Overview", icon: LayoutDashboard },
  { id: "backup", group: "silo", label: "Backup", icon: CloudUpload },
  { id: "verify", group: "silo", label: "Test backup", icon: SearchCheck },
  { id: "recovery", group: "silo", label: "Recovery code", icon: LifeBuoy },
  { id: "keys", group: "silo", label: "Unlocking", icon: KeyRound },
  { id: "devices", group: "silo", label: "Devices and activity", icon: Laptop },
  { id: "protected", group: "silo", label: "Auto-import folders", icon: FolderHeart },
  { id: "advanced", group: "silo", label: "Advanced", icon: Wrench },
  { id: "general", group: "app", label: "General", icon: SlidersHorizontal },
  { id: "browser", group: "app", label: "Browser extension", icon: Globe },
  { id: "updates", group: "app", label: "Updates and about", icon: DownloadCloud },
] as const;

export type SettingsSectionId = (typeof SECTIONS)[number]["id"];

export function SettingsPanel(props: Props) {
  const platform = platformStrings(props.os);
  const {
    busy,
    autoUpdateEnabled,
    onAutoUpdateEnabled,
    backgroundUpdate,
    onUpdateFailedAfterLock,
    securityKeys,
    newKeyLabel,
    onNewKeyLabel,
    fidoProgress,
    keyAddSuccess,
    onAddKey,
    onRemoveKey,
    onRotateKey,
    rotationPending,
    onResumeRotation,
    onRenameKey,
    devices,
    onRenameDevice,
    platformAvailable,
    silo,
    onRenameSilo,
    onForgetSilo,
    onSwitchSilo,
    recovery,
    recoveryCode,
    onGenerateRecovery,
    onDisableRecovery,
    onCopyRecoveryCode,
    onDismissRecoveryCode,
    autoLockMinutes,
    onAutoLockMinutes,
    siloAutoLockMinutes,
    onSiloAutoLockMinutes,
    sync,
    fullCopy,
    section,
    onSection,
    backupPanel,
    verifyPanel,
  } = props;

  // A silo whose only way in is sealed to this machine is one dead
  // motherboard away from gone, and nothing else on this screen would say so.
  const hasPortableKey = securityKeys.some((k) => !k.platform);
  /// Whether this silo is organisation-administered, which is the only case
  /// where "add as an organisation key" means anything and the only case it
  /// is offered: on a personal silo the backend would refuse it anyway.
  const orgControlled = securityKeys.some((k) => k.policy === "org");
  const [addAsOrganisation, setAddAsOrganisation] = useState(false);
  const [renamingKey, setRenamingKey] = useState<string | null>(null);
  const [keyLabelDraft, setKeyLabelDraft] = useState("");

  const commitKeyRename = (credentialId: string) => {
    const label = keyLabelDraft.trim();
    if (!label) return;
    onRenameKey(credentialId, label);
    setRenamingKey(null);
  };

  const [renamingDevice, setRenamingDevice] = useState<string | null>(null);
  const [deviceLabelDraft, setDeviceLabelDraft] = useState("");

  const commitDeviceRename = (deviceId: string) => {
    onRenameDevice(deviceId, deviceLabelDraft.trim());
    setRenamingDevice(null);
  };

  const updater = useUpdater(backgroundUpdate, onUpdateFailedAfterLock);

  /// Each group's heading is drawn once, above the first item that carries
  /// it, so the rail reads as two blocks: this silo, then the app.
  let lastGroup = "";

  return (
    <div className="settings-view">
      <ViewHeader icon={Settings2} title="Settings" subtitle={`${silo.name}, and the app itself`} />
      <div className="settings-body">
        <nav className="view-rail" aria-label="Settings sections">
          {SECTIONS.map((item) => {
            const Icon = item.icon;
            const heading = item.group === lastGroup ? null : (lastGroup = item.group);
            return (
              <div key={item.id}>
                {heading && (
                  <div className="view-rail-heading">
                    {heading === "silo" ? silo.name : "App, every silo"}
                  </div>
                )}
                <button
                  type="button"
                  className={`view-rail-item${section === item.id ? " active" : ""}`}
                  aria-current={section === item.id ? "page" : undefined}
                  onClick={() => onSection(item.id)}
                >
                  <Icon size={14} aria-hidden className="settings-rail-icon" />
                  <span className="view-rail-label">{item.label}</span>
                  {item.id === "updates" && updater.state.phase === "available" && (
                    <span className="tab-badge tab-badge-update rail-update-badge">New</span>
                  )}
                </button>
              </div>
            );
          })}
        </nav>

        <div className="settings-pane">
        {section === "overview" && (
          <OverviewPanel
            os={props.os}
            busy={busy}
            silo={silo}
            sync={sync}
            recovery={recovery}
            hasPortableKey={hasPortableKey}
            fullCopy={fullCopy}
            onGo={(target) => onSection(target)}
            onRenameSilo={onRenameSilo}
            onSwitchSilo={onSwitchSilo}
          />
        )}

        {section === "backup" && backupPanel}

        {section === "verify" && verifyPanel}

        {section === "recovery" && (
          <div className="panel-section">
            <h3>
              <LifeBuoy size={16} />
              Recovery code
            </h3>
            <p>
              A long code that opens the silo on any computer when every key is lost. Write it down
              and keep it safe: anyone who has it can open the silo.
            </p>
            {recoveryCode ? (
              <div className="recovery-reveal">
                <p className="hint is-error">
                  <AlertTriangle size={14} />
                  Shown once. After you close this, the only way to get a code is to make a new
                  one.
                </p>
                <code className="recovery-code">{recoveryCode}</code>
                {/* The consequence belongs next to the code itself, where
                    someone is deciding how carefully to store it, rather
                    than only on the setup screen they saw once. */}
                <div className="consequence">
                  <h3>
                    <AlertTriangle size={15} />
                    This code and your keys are the only way in
                  </h3>
                  <p>
                    If you lose all of them, the files are lost for good. We
                    keep no copy of your key and cannot open the silo for you.
                    Keep this where you keep your passport.
                  </p>
                </div>
                <div className="actions">
                  <button type="button" onClick={onCopyRecoveryCode}>
                    <Copy size={15} />
                    Copy
                  </button>
                  <button type="button" className="secondary" onClick={onDismissRecoveryCode}>
                    I&apos;ve written it down
                  </button>
                </div>
              </div>
            ) : (
              <>
                <p className={`hint${recovery.enabled ? " success-msg" : ""}`}>
                  {recovery.enabled ? (
                    <>
                      <CheckCircle2 size={14} />
                      A recovery code is active
                      {recovery.created_at ? `, created ${formatDay(recovery.created_at)}` : ""}
                      .
                    </>
                  ) : (
                    "No recovery code yet. Losing every key would mean losing the silo."
                  )}
                </p>
                <div className="actions">
                  <button type="button" disabled={busy} onClick={onGenerateRecovery}>
                    {recovery.enabled ? "Replace the code" : "Create a recovery code"}
                  </button>
                </div>
                {recovery.enabled && (
                  <p className="hint">
                    Replace it if you think someone has seen the paper copy. The old code stops
                    working, except on a never-delete copy, which keeps it.
                  </p>
                )}
              </>
            )}

            <EmergencyKitPanel
              busy={busy}
              siloId={silo.id}
              siloName={silo.name}
              freshCode={recoveryCode}
            />
          </div>
        )}

        {section === "keys" && (
          <>
          {rotationPending && (
            <div className="panel-section">
              <p className="hint is-error">
                <AlertTriangle size={14} />
                Replacing the encryption key was started and never finished. Syncing fails until
                it is.
              </p>
              <div className="actions">
                <button type="button" disabled={busy} onClick={() => onSection("advanced")}>
                  Finish it under Advanced
                </button>
              </div>
            </div>
          )}
          <div className="panel-section">
            <h3>
              <KeyRound size={16} />
              Keys
            </h3>
            <p>
              Most USB and NFC security keys work (YubiKey, Nitrokey, SoloKeys), and so does{" "}
              {platform.builtIn} on this computer. Each one unlocks the silo on its own.
            </p>
            {securityKeys.length === 0 ? (
              <p className="hint empty-state-row">
                <Inbox size={14} />
                The key list could not be read. Try reopening this page.
              </p>
            ) : (
              <ul className="key-list">
                {securityKeys.map((k) =>
                  renamingKey === k.credential_id ? (
                    <li key={k.credential_id} className="key-list-item">
                      <input
                        type="text"
                        className="key-rename-input"
                        value={keyLabelDraft}
                        autoFocus
                        maxLength={64}
                        aria-label={`Name for ${k.label || `slot ${k.key_slot}`}`}
                        onChange={(e) => setKeyLabelDraft(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Enter") commitKeyRename(k.credential_id);
                          if (e.key === "Escape") setRenamingKey(null);
                        }}
                      />
                      <div className="key-list-actions">
                        <button
                          type="button"
                          className="link"
                          disabled={busy || !keyLabelDraft.trim()}
                          onClick={() => commitKeyRename(k.credential_id)}
                        >
                          Save
                        </button>
                        <button type="button" className="link" onClick={() => setRenamingKey(null)}>
                          Cancel
                        </button>
                      </div>
                    </li>
                  ) : (
                  <li
                    key={k.credential_id}
                    className="key-list-item"
                    // For telling two keys apart when asking for help; not
                    // something to read on every visit.
                    title={`Slot ${k.key_slot}, id ${k.credential_id.slice(0, 12)}`}
                  >
                    <div>
                      <strong>{securityKeyDisplayName(k, props.os)}</strong>
                      {/* Never hidden, on any device. A key the person at this
                          computer cannot remove is something they are entitled
                          to see named for what it is. */}
                      {k.policy === "org" && (
                        <span className="key-badge" title="Administered by an organisation">
                          <Building2 size={12} aria-hidden />
                          Organisation
                        </span>
                      )}
                      <span className="hint">
                        {" "}
                        ·{" "}
                        {!usableHere(k)
                          ? "another device"
                          : k.platform
                            ? "this computer only"
                            : "portable"}
                      </span>
                    </div>
                    <div className="key-list-actions">
                      {/* A name is the only thing telling two keys apart at a
                          glance; the rest of the row is a slot number and
                          twelve hex characters. */}
                      <button
                        type="button"
                        className="link"
                        disabled={busy}
                        onClick={() => {
                          setKeyLabelDraft(k.label || securityKeyDisplayName(k, props.os));
                          setRenamingKey(k.credential_id);
                        }}
                      >
                        Rename
                      </button>
                      <button
                        type="button"
                        className="link"
                        disabled={busy || securityKeys.length <= 1}
                        title={
                          securityKeys.length <= 1
                            ? "The last key cannot be removed: nothing would open the silo."
                            : k.policy === "org"
                              ? "An organisation administers this key. Removing it asks for one of the organisation's keys."
                              : undefined
                        }
                        onClick={() => onRemoveKey(k.credential_id)}
                      >
                        Remove
                      </button>
                    </div>
                  </li>
                  )
                )}
              </ul>
            )}
            <div className="key-add-panel">
              <p className="hint">To add another key:</p>
              <ol className="hint key-add-steps">
                <li>Plug in the new key.</li>
                <li>Give it a label, if you want one.</li>
                <li>Click Add. {platform.osName} asks for two touches on that key.</li>
              </ol>
              <p className="hint">Wait for the confirmation before clicking again.</p>
              {!hasPortableKey && !recovery.enabled && securityKeys.length > 0 && (
                <p className="hint is-error">
                  <AlertTriangle size={14} />
                  Everything that opens this silo works only on this computer. If the computer
                  fails, the silo is lost with it. Add a portable security key or a recovery code.
                </p>
              )}
              {fidoProgress && (
                <p className="fido-live" role="status">
                  {fidoProgress}
                </p>
              )}
              {keyAddSuccess && !busy && (
                <p className="success-msg" role="status">
                  {keyAddSuccess}
                </p>
              )}
              {/* Only on an administered silo, where a company needs a spare:
                  losing the only organisation key leaves the silo administered
                  by nobody. Enrolling one asks for an existing organisation
                  key first, so this is not a way for anyone else to add one. */}
              {orgControlled && (
                <label className="confirm-option org-option">
                  <input
                    type="checkbox"
                    checked={addAsOrganisation}
                    disabled={busy}
                    onChange={(e) => setAddAsOrganisation(e.target.checked)}
                  />
                  <span>
                    Enrol as an organisation key
                    <span className="hint">
                      A spare to keep in the company safe. You will be asked for an existing
                      organisation key first. {platform.builtIn} cannot be one, because it works
                      only on this computer.
                    </span>
                  </span>
                </label>
              )}
              <div className="inline-form explorer-new-folder">
                <input
                  type="text"
                  placeholder="Label (optional)"
                  value={newKeyLabel}
                  disabled={busy}
                  onChange={(e) => onNewKeyLabel(e.target.value)}
                />
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => onAddKey("security-key", addAsOrganisation)}
                >
                  {busy && fidoProgress && <span className="spinner" aria-hidden />}
                  {busy && fidoProgress ? "Waiting…" : "Add security key"}
                </button>
                {platformAvailable && !addAsOrganisation && (
                  <button
                    type="button"
                    className="secondary"
                    disabled={busy}
                    onClick={() => onAddKey("this-device", false)}
                  >
                    Add {platform.builtIn}
                  </button>
                )}
              </div>
            </div>
          </div>
          <div className="panel-section">
            <h3>
              <Timer size={16} />
              Auto-lock
            </h3>
            <div className="settings-row">
              <label className="settings-row-label" htmlFor="auto-lock-silo">
                Lock <strong>{silo.name}</strong> after
              </label>
              <select
                id="auto-lock-silo"
                className="auto-lock-select"
                value={siloAutoLockMinutes ?? "default"}
                disabled={busy}
                onChange={(e) =>
                  onSiloAutoLockMinutes(
                    e.target.value === "default" ? null : Number.parseInt(e.target.value, 10),
                  )
                }
              >
                <option value="default">the default ({formatMinutes(autoLockMinutes)})</option>
                {AUTO_LOCK_OPTIONS_MINUTES.map((minutes) => (
                  <option key={minutes} value={minutes}>
                    {formatMinutes(minutes)}
                  </option>
                ))}
              </select>
            </div>
            <p className="hint">
              Each silo counts on its own, so working in one does not hold another open. The
              default is under General.
            </p>
          </div>
          </>
        )}

        {section === "devices" && (
          <>
          <div className="panel-section">
            <h3>
              <Laptop size={16} />
              Devices and activity
            </h3>
            <p>
              Every device that has changed something in this silo. A name you give one here is
              shown on every device.
            </p>
            <ul className="key-list">
              {devices.map((device) =>
                renamingDevice === device.id ? (
                  <li key={device.id} className="key-list-item">
                    <input
                      type="text"
                      className="key-rename-input"
                      value={deviceLabelDraft}
                      autoFocus
                      maxLength={60}
                      placeholder={device.system_name ?? "Laptop, Desktop, Work machine…"}
                      aria-label={`Name for device ${device.id.slice(0, 8)}`}
                      onChange={(e) => setDeviceLabelDraft(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") commitDeviceRename(device.id);
                        if (e.key === "Escape") setRenamingDevice(null);
                      }}
                    />
                    <div className="key-list-actions">
                      <button
                        type="button"
                        className="link"
                        disabled={busy}
                        onClick={() => commitDeviceRename(device.id)}
                      >
                        Save
                      </button>
                      <button
                        type="button"
                        className="link"
                        onClick={() => setRenamingDevice(null)}
                      >
                        Cancel
                      </button>
                    </div>
                  </li>
                ) : (
                  <li key={device.id} className="key-list-item">
                    <div>
                      <strong>
                        {device.label || device.system_name || "Unnamed device"}
                      </strong>
                      <span className="hint">
                        {device.is_this_device ? " · this computer" : ""}
                        {device.platform ? ` · ${device.platform}` : ""}
                        {/* Only when a person renamed it: otherwise the
                            heading already is the computer name and this
                            would print it twice. */}
                        {device.label && device.system_name ? ` · ${device.system_name}` : ""}
                        {" · "}
                        {device.operations === 1 ? "1 change" : `${device.operations} changes`}
                        {device.last_change_at > 0
                          ? ` · last on ${formatDate(device.last_change_at)}`
                          : ""}
                      </span>
                    </div>
                    <div className="key-list-actions">
                      <button
                        type="button"
                        className="link"
                        disabled={busy}
                        onClick={() => {
                          setDeviceLabelDraft(device.label ?? device.system_name ?? "");
                          setRenamingDevice(device.id);
                        }}
                      >
                        Rename
                      </button>
                    </div>
                  </li>
                )
              )}
            </ul>
            {/* Said plainly rather than left to be discovered: a list of
                devices with no way to remove one reads as a missing button
                until you know where the door actually is. */}
            <p className="hint">
              To stop a device opening the silo, remove its key under Unlocking. The device stays
              in this list with the changes it made.
            </p>
          </div>
          <ActivityList devices={devices} />
          </>
        )}

        {section === "protected" && <ProtectedFoldersPanel />}

        {section === "advanced" && (
          <>
            {securityKeys.length > 0 && (
              <RotateKeyPanel
                os={props.os}
                busy={busy}
                keys={securityKeys}
                progress={fidoProgress}
                onRotate={onRotateKey}
                pending={rotationPending}
                onResume={onResumeRotation}
                archiveTargets={props.archiveTargets}
              />
            )}

            {recovery.enabled && (
              <div className="panel-section">
                <h3>
                  <LifeBuoy size={16} />
                  Turn off the recovery code
                </h3>
                <p>The code you wrote down stops working, and only your keys open this silo.</p>
                <div className="actions">
                  <button
                    type="button"
                    className="danger"
                    disabled={busy}
                    onClick={onDisableRecovery}
                  >
                    Turn off the recovery code
                  </button>
                </div>
              </div>
            )}

            <div className="panel-section panel-section-danger">
              <h3 className="is-danger">
                <AlertTriangle size={16} />
                Remove this silo
              </h3>
              <p>
                Takes <strong>{silo.name}</strong> out of the list on this computer. The folder
                stays on disk unless you choose otherwise, so you can add it back later.
              </p>
              <div className="actions">
                <button type="button" className="danger" disabled={busy} onClick={onForgetSilo}>
                  Remove this silo
                </button>
              </div>
            </div>
          </>
        )}

        {(section === "general" || section === "browser" || section === "updates") && (
          <AppSettingsSection
            section={section}
            os={props.os}
            busy={busy}
            updater={updater}
            autoUpdateEnabled={autoUpdateEnabled}
            onAutoUpdateEnabled={onAutoUpdateEnabled}
            defaultAutoLockMinutes={autoLockMinutes}
            onDefaultAutoLockMinutes={onAutoLockMinutes}
            siloHasKeys={securityKeys.length > 0}
          />
        )}
        </div>
      </div>
    </div>
  );
}
