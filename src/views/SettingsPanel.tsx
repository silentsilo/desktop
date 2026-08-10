import { useEffect, useState, type ReactNode } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  CloudUpload,
  Copy,
  DownloadCloud,
  ExternalLink,
  HardDrive,
  Inbox,
  Info,
  KeyRound,
  Laptop,
  LifeBuoy,
  Power,
  SearchCheck,
  Timer,
} from "lucide-react";
import { FolderHeart, History, Settings2 } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
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
import { formatBytes, formatDate, formatDay } from "../lib/format";
import { securityKeyDisplayName } from "../lib/keyName";
import { AUTO_LOCK_OPTIONS_MINUTES } from "../lib/types";
import { checkForUpdate, installUpdateAndRelaunch } from "../lib/updater";
import { readAutostart, writeAutostart, type AutostartStatus } from "../lib/autostart";
import { formatAppError } from "../lib/errors";
import { ActivityList } from "./ActivityList";
import { ProtectedFoldersPanel } from "./ProtectedFolders";
import type { Update } from "@tauri-apps/plugin-updater";

type Props = {
  busy: boolean;
  /** Whether the daily scheduled update check is on. */
  autoUpdateEnabled: boolean;
  onAutoUpdateEnabled: (on: boolean) => void;
  /** An update the scheduled check already found, so this panel can offer
   * the install without a second request. */
  backgroundUpdate: { version: string; update: Update } | null;
  securityKeys: SecurityKeyInfo[];
  newKeyLabel: string;
  onNewKeyLabel: (v: string) => void;
  fidoProgress: string | null;
  keyAddSuccess: string | null;
  onAddKey: (authenticator: Authenticator) => void;
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
  /** The fallback a silo follows when it has no timeout of its own. Shown
   * as a label, not set here: this page belongs to one silo. */
  autoLockMinutes: number;
  /** This silo's own timeout, or null to follow the default. */
  siloAutoLockMinutes: number | null;
  onSiloAutoLockMinutes: (minutes: number | null) => void;
  /** Which section is open. Held by the caller so the sidebar's storage
   * figure can open Settings straight at Backup. */
  section: SettingsSectionId;
  onSection: (id: SettingsSectionId) => void;
  /** The backup page, hosted here rather than configured here: what it
   * needs comes from the app shell, and Settings only gives it a place. */
  backupPanel: ReactNode;
  /** The copies page, same arrangement as the backup page. */
  copiesPanel: ReactNode;
  /** The verification page, same arrangement as the backup page. */
  verifyPanel: ReactNode;
};

/**
 * The sections, in rail order. One entry per thing you can change, which is
 * what the left column lists and the right column shows one of.
 */
const SECTIONS = [
  { id: "auto-lock", group: "Security", label: "Auto-lock", icon: Timer },
  { id: "keys", group: "Security", label: "Security keys", icon: KeyRound },
  { id: "recovery", group: "Security", label: "Recovery code", icon: LifeBuoy },
  { id: "silo", group: "This silo", label: "This silo", icon: HardDrive },
  { id: "devices", group: "This silo", label: "Devices", icon: Laptop },
  { id: "activity", group: "This silo", label: "Activity", icon: History },
  { id: "protected", group: "This silo", label: "Protected folders", icon: FolderHeart },
  { id: "backup", group: "This silo", label: "Backup", icon: CloudUpload },
  { id: "copies", group: "This silo", label: "Copies", icon: Copy },
  { id: "verify", group: "This silo", label: "Verification", icon: SearchCheck },
  { id: "startup", group: "Application", label: "Startup", icon: Power },
  { id: "updates", group: "Application", label: "Updates", icon: DownloadCloud },
  { id: "about", group: "Application", label: "About", icon: Info },
  { id: "danger", group: "Application", label: "Danger zone", icon: AlertTriangle },
] as const;

export type SettingsSectionId = (typeof SECTIONS)[number]["id"];

function formatMinutes(minutes: number): string {
  if (minutes < 60) return `${minutes} min`;
  const hours = minutes / 60;
  return hours === 1 ? "1 hour" : `${hours} hours`;
}

export function SettingsPanel(props: Props) {
  const {
    busy,
    autoUpdateEnabled,
    onAutoUpdateEnabled,
    backgroundUpdate,
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
    siloAutoLockMinutes,
    onSiloAutoLockMinutes,
    section,
    onSection,
    backupPanel,
    copiesPanel,
    verifyPanel,
  } = props;

  // A silo whose only way in is sealed to this machine is one dead
  // motherboard away from gone, and nothing else on this screen would say so.
  const hasPortableKey = securityKeys.some((k) => !k.platform);
  const [siloName, setSiloName] = useState(silo.name);
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

  // Null until the first read comes back, and again if it fails: the
  // checkbox has no honest state to show before the OS has answered.
  const [autostart, setAutostart] = useState<AutostartStatus | null>(null);
  const [autostartError, setAutostartError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void readAutostart()
      .then((status) => {
        if (!cancelled) setAutostart(status);
      })
      .catch(() => {
        if (!cancelled) setAutostart(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const toggleAutostart = async (on: boolean) => {
    const previous = autostart;
    setAutostartError(null);
    setAutostart((s) => (s ? { ...s, enabled: on } : s));
    try {
      await writeAutostart(on);
    } catch (e) {
      setAutostart(previous);
      setAutostartError(formatAppError(e));
    }
  };

  type UpdateState =
    | { phase: "idle" }
    | { phase: "checking" }
    | { phase: "up-to-date" }
    | { phase: "available"; version: string; update: Update }
    | {
        phase: "installing";
        version: string;
        /** Bytes down so far, and the whole size when the server said one. */
        downloaded: number;
        contentLength: number | null;
      }
    | { phase: "error"; message: string };

  const [updateState, setUpdateState] = useState<UpdateState>({ phase: "idle" });

  // An update the daily check found lands here as the panel's initial
  // state, so opening Settings shows it without another request. Any state
  // the user has since driven (checking, installing) wins over it.
  useEffect(() => {
    if (backgroundUpdate && updateState.phase === "idle") {
      setUpdateState({
        phase: "available",
        version: backgroundUpdate.version,
        update: backgroundUpdate.update,
      });
    }
  }, [backgroundUpdate, updateState.phase]);

  const handleCheckForUpdate = async () => {
    setUpdateState({ phase: "checking" });
    try {
      const result = await checkForUpdate();
      setUpdateState(
        result.available
          ? { phase: "available", version: result.version, update: result.update }
          : { phase: "up-to-date" },
      );
    } catch (e) {
      setUpdateState({ phase: "error", message: formatAppError(e) });
    }
  };

  const handleInstallUpdate = async (update: Update, version: string) => {
    setUpdateState({ phase: "installing", version, downloaded: 0, contentLength: null });
    try {
      await installUpdateAndRelaunch(update, (downloaded, contentLength) => {
        setUpdateState({ phase: "installing", version, downloaded, contentLength });
      });
    } catch (e) {
      setUpdateState({ phase: "error", message: formatAppError(e) });
    }
  };

  /// Each group's heading is drawn once, above the first item that carries
  /// it, so the rail reads as three blocks rather than a flat list of six.
  let lastGroup = "";

  return (
    <div className="settings-view">
      <ViewHeader
        icon={Settings2}
        title="Settings"
        subtitle={`Keys, recovery, backup and updates for ${silo.name}`}
      />
      <div className="settings-body">
        <nav className="view-rail" aria-label="Settings sections">
        {SECTIONS.map((item) => {
          const Icon = item.icon;
          const heading = item.group === lastGroup ? null : (lastGroup = item.group);
          return (
            <div key={item.id}>
              {heading && <div className="view-rail-heading">{heading}</div>}
              <button
                type="button"
                className={`view-rail-item${section === item.id ? " active" : ""}`}
                aria-current={section === item.id ? "page" : undefined}
                onClick={() => onSection(item.id)}
              >
                <Icon size={14} aria-hidden className="settings-rail-icon" />
                <span className="view-rail-label">{item.label}</span>
              </button>
            </div>
          );
        })}
      </nav>

      <div className="settings-pane">
        {section === "auto-lock" && (
          <div className="panel-section">
            <h3>
              <Timer size={16} />
              Auto-lock
            </h3>
            <p>
              Time counts against each silo on its own, so working in one does not hold another
              open. This page is <strong>{silo.name}</strong>: every other silo keeps its own
              setting.
            </p>
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
                <option value="default">
                  the default ({formatMinutes(autoLockMinutes)})
                </option>
                {AUTO_LOCK_OPTIONS_MINUTES.map((minutes) => (
                  <option key={minutes} value={minutes}>
                    {formatMinutes(minutes)}
                  </option>
                ))}
              </select>
            </div>
          </div>
        )}

        {section === "keys" && (
          <>
          <div className="panel-section">
            <h3>
              <KeyRound size={16} />
              Security keys
            </h3>
            <p>
              Any FIDO2 hardware key with hmac-secret works (YubiKey, Nitrokey, SoloKeys, and
              others), as does this computer&apos;s built-in Windows Hello. Each one can unlock
              the silo on its own.
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
                  <li key={k.credential_id} className="key-list-item">
                    <div>
                      <strong>{securityKeyDisplayName(k)}</strong>
                      <span className="hint">
                        {" "}
                        · {k.platform ? "this computer only" : "portable"}
                        {/* Only when a person named the key: the fallback
                            name already carries the slot, and printing it
                            twice on one row read as two different keys. */}
                        {k.label ? ` · slot ${k.key_slot}` : ""} · {k.credential_id.slice(0, 12)}…
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
                          setKeyLabelDraft(k.label || securityKeyDisplayName(k));
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
                <li>Click Add. Windows asks for two touches on that key.</li>
              </ol>
              <p className="hint">Wait for the confirmation before clicking again.</p>
              {!hasPortableKey && securityKeys.length > 0 && (
                <p className="hint is-error">
                  <AlertTriangle size={14} />
                  Everything that opens this silo is sealed to this computer. If it fails, the
                  silo goes with it. Add a portable security key or a recovery code.
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
              <div className="inline-form explorer-new-folder">
                <input
                  type="text"
                  placeholder="Label (optional)"
                  value={newKeyLabel}
                  disabled={busy}
                  onChange={(e) => onNewKeyLabel(e.target.value)}
                />
                <button type="button" disabled={busy} onClick={() => onAddKey("security-key")}>
                  {busy && fidoProgress && <span className="spinner" aria-hidden />}
                  {busy && fidoProgress ? "Waiting…" : "Add security key"}
                </button>
                {platformAvailable && (
                  <button
                    type="button"
                    className="secondary"
                    disabled={busy}
                    onClick={() => onAddKey("this-device")}
                  >
                    Add Windows Hello
                  </button>
                )}
              </div>
            </div>
          </div>

          {securityKeys.length > 0 && (
            <RotateKeyPanel
              busy={busy}
              keys={securityKeys}
              progress={fidoProgress}
              onRotate={onRotateKey}
              pending={rotationPending}
              onResume={onResumeRotation}
            />
          )}
          </>
        )}

        {section === "recovery" && (
          <>
          <div className="panel-section">
            <h3>
              <LifeBuoy size={16} />
              Recovery code
            </h3>
            <p>
              One long code, generated here, that opens the silo when every key is gone, even on
              a computer that has never seen it. Write it down and keep it somewhere physical.
              Anyone holding it can open the silo, so treat it like the key itself.
            </p>
            {recoveryCode ? (
              <div className="recovery-reveal">
                <p className="hint is-error">
                  <AlertTriangle size={14} />
                  Shown once. Close this and it is gone, and generating a new one is the only
                  way to get another.
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
                    Lose all of them and the files are gone, permanently. We
                    cannot open your silo, and no support request can: there
                    is no backdoor and no copy of your key on our side. Keep
                    this where you would keep a passport.
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
                  {recovery.enabled && (
                    <button
                      type="button"
                      className="danger"
                      disabled={busy}
                      onClick={onDisableRecovery}
                    >
                      Turn off
                    </button>
                  )}
                </div>
                {recovery.enabled && (
                  <p className="hint">
                    Replacing invalidates the old code everywhere. Do that if you think the paper
                    copy has been seen.
                  </p>
                )}
              </>
            )}

            <EmergencyKitPanel busy={busy} siloName={silo.name} freshCode={recoveryCode} />
          </div>
          </>
        )}

        {section === "activity" && <ActivityList devices={devices} />}

        {section === "protected" && <ProtectedFoldersPanel />}

        {section === "devices" && (
          <div className="panel-section">
            <h3>
              <Laptop size={16} />
              Devices
            </h3>
            <p>
              Every device that has changed something in this silo, read from the operation log
              itself. Each one reports its computer name and system when it opens the silo; rename
              one here and your name is used instead, on every device.
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
                        {device.label || device.system_name || `Device ${device.id.slice(0, 8)}`}
                      </strong>
                      <span className="hint">
                        {device.is_this_device ? " · this device" : ""}
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
              A device is not removed from here. What lets a device in is its security key, so
              taking one away is done on the Security keys page; the device stays in this list as a
              record of what it changed.
            </p>
          </div>
        )}

        {section === "silo" && (
          <div className="panel-section">
            <h3>
              <HardDrive size={16} />
              This silo
            </h3>
            <p>
              The keys, recovery code and auto-lock on this page belong to{" "}
              <strong>{silo.name}</strong> alone; its backup lives on the Backup page. Your other
              silos are untouched.
            </p>
            <p className="hint">{silo.path}</p>
            <div className="inline-form explorer-new-folder">
              <input
                type="text"
                value={siloName}
                disabled={busy}
                onChange={(e) => setSiloName(e.target.value)}
                aria-label="Silo name"
              />
              <button
                type="button"
                disabled={busy || siloName.trim() === silo.name || !siloName.trim()}
                onClick={() => onRenameSilo(siloName.trim())}
              >
                Rename
              </button>
            </div>
            <p className="hint">
              Only the label changes. The folder keeps its name, so backups and shortcuts pointing
              at it keep working.
            </p>
            <div className="actions">
              <button type="button" className="secondary" disabled={busy} onClick={onSwitchSilo}>
                Switch to another silo
              </button>
            </div>
          </div>
        )}

        {section === "backup" && backupPanel}

        {section === "copies" && copiesPanel}

        {section === "verify" && verifyPanel}

        {section === "startup" && (
          <div className="panel-section">
            <h3>
              <Power size={16} />
              Startup
            </h3>
            <p>
              SilentSilo stays in the notification area once started, and closing the window only
              hides it. That is what lets the Explorer right-click actions land in the silo you
              already unlocked, and what keeps backup running on its schedule.
            </p>
            <label className="s3-checkbox">
              <input
                type="checkbox"
                checked={autostart?.enabled ?? false}
                disabled={busy || !autostart?.supported}
                onChange={(e) => void toggleAutostart(e.target.checked)}
              />
              <span>
                Start SilentSilo when I sign in to Windows
                <span className="hint">
                  It starts to the notification area with no window and nothing unlocked. Your
                  security key is still needed before a silo opens.
                </span>
              </span>
            </label>
            {autostart && !autostart.supported && (
              <p className="hint">Not available on this system.</p>
            )}
            {autostartError && <p className="hint is-error">{autostartError}</p>}
            <p className="hint">
              Windows lists this under Startup apps in Task Manager. Turning it off there and
              turning it off here are the same thing.
            </p>
          </div>
        )}

        {section === "updates" && (
          <div className="panel-section">
            <h3>
              <DownloadCloud size={16} />
              Updates
            </h3>
            <p>Current version: v{__APP_VERSION__}</p>
            {updateState.phase === "available" && (
              <p className="hint success-msg">
                <CheckCircle2 size={14} />
                Version {updateState.version} is available.
              </p>
            )}
            {updateState.phase === "up-to-date" && (
              <p className="hint success-msg">
                <CheckCircle2 size={14} />
                You are on the latest version.
              </p>
            )}
            {updateState.phase === "installing" && (
              <div className="progress-row" role="status">
                <p className="hint">
                  Downloading v{updateState.version}
                  {updateState.contentLength
                    ? `: ${formatBytes(updateState.downloaded)} of ${formatBytes(updateState.contentLength)}`
                    : updateState.downloaded > 0
                      ? `: ${formatBytes(updateState.downloaded)} so far`
                      : ""}
                  . The app restarts on its own when it is done.
                </p>
                {updateState.contentLength !== null && updateState.contentLength > 0 && (
                  <div className="progress-track">
                    <div
                      className="progress-fill"
                      style={{
                        width: `${Math.min(100, (updateState.downloaded / updateState.contentLength) * 100)}%`,
                      }}
                    />
                  </div>
                )}
              </div>
            )}
            {updateState.phase === "error" && <p className="hint is-error">{updateState.message}</p>}
            <div className="actions">
              {updateState.phase === "available" ? (
                <button
                  type="button"
                  onClick={() => void handleInstallUpdate(updateState.update, updateState.version)}
                >
                  Download &amp; install
                </button>
              ) : (
                <button
                  type="button"
                  className="secondary"
                  disabled={updateState.phase === "checking" || updateState.phase === "installing"}
                  onClick={() => void handleCheckForUpdate()}
                >
                  {updateState.phase === "checking" ? "Checking…" : "Check for updates"}
                </button>
              )}
            </div>
            <label className="s3-checkbox update-auto-toggle">
              <input
                type="checkbox"
                checked={autoUpdateEnabled}
                onChange={(e) => onAutoUpdateEnabled(e.target.checked)}
              />
              <span>
                Check for updates automatically
                <span className="hint">
                  Once a day at most. The request carries the app version and
                  platform, nothing that identifies you or this computer.
                  Turning this off means security fixes only reach you when
                  you check by hand.
                </span>
              </span>
            </label>
          </div>
        )}

        {section === "about" && (
          <div className="panel-section">
            <h3>
              <Info size={16} />
              About SilentSilo
            </h3>
            <p>
              Version {__APP_VERSION__}. An encrypted vault for files and passwords, unlocked by a
              FIDO2 security key or Windows Hello, backed up to storage you control.
            </p>
            <dl className="backup-config">
              <div className="backup-config-row">
                <dt>Publisher</dt>
                <dd>Software Hive S.R.L.</dd>
              </div>
              <div className="backup-config-row">
                <dt>License</dt>
                <dd>GNU AGPL v3, provided as is, without warranty. The source code is public.</dd>
              </div>
            </dl>
            <div className="actions">
              <button
                type="button"
                className="secondary"
                onClick={() => void openUrl("https://silentsilo.com")}
              >
                <ExternalLink size={14} />
                silentsilo.com
              </button>
              <button
                type="button"
                className="secondary"
                onClick={() => void openUrl("https://github.com/silentsilo/desktop")}
              >
                <ExternalLink size={14} />
                Source code
              </button>
            </div>
            <p className="hint">
              © {new Date().getFullYear()} Software Hive S.R.L. SilentSilo is a trademark of
              Software Hive S.R.L.
            </p>
          </div>
        )}

        {section === "danger" && (
          <div className="panel-section panel-section-danger">
            <h3 className="is-danger">
              <AlertTriangle size={16} />
              Danger zone
            </h3>
            <p>
              Removing <strong>{silo.name}</strong> from this computer takes it out of the list.
              The folder is left alone unless you say otherwise, so this is reversible: add the
              folder back and the silo returns.
            </p>
            <div className="actions">
              <button type="button" className="danger" disabled={busy} onClick={onForgetSilo}>
                Remove this silo
              </button>
            </div>
          </div>
        )}
        </div>
      </div>
    </div>
  );
}
