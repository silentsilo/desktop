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
  SquareTerminal,
  Timer,
  Wrench,
} from "lucide-react";
import { FolderHeart, Settings2 } from "lucide-react";
import { RotateKeyPanel } from "./RotateKeyPanel";
import { EmergencyKitPanel } from "./EmergencyKitPanel";
import { ViewHeader } from "../components/ViewHeader";
import type {
  AuditStatus,
  Authenticator,
  DeviceInfo,
  RecoveryStatus,
  SecurityKeyInfo,
  Silo,
} from "../lib/types";
import { formatDate, formatDay } from "../lib/format";
import { securityKeyDisplayName, usableHere } from "../lib/keyName";
import { AUTO_LOCK_OPTIONS_MINUTES } from "../lib/types";
import { AuditLogPanel } from "./AuditLogPanel";
import { ProtectedFoldersPanel } from "./ProtectedFolders";
import type { Update } from "@tauri-apps/plugin-updater";
import type { SyncIndicator } from "../layout/AppShell";
import { AppSettingsSection, formatMinutes, useUpdater } from "./settings/AppSettings";
import { OverviewPanel } from "./settings/OverviewPanel";
import { SettingList, SettingRow } from "../components/Setting";
import { t, tx, useLocale } from "../i18n";

type Props = {
  /** Whether the open silo keeps an activity log, for the overview. */
  auditLog: AuditStatus | null;
  onAuditChanged: (status: AuditStatus) => void;
  /** The Activity page, outside Settings. */
  onOpenActivity: () => void;
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
  { id: "overview", group: "silo", label: "settings.overview", icon: LayoutDashboard },
  { id: "backup", group: "silo", label: "settings.backup", icon: CloudUpload },
  { id: "verify", group: "silo", label: "settings.verify", icon: SearchCheck },
  { id: "recovery", group: "silo", label: "settings.recovery", icon: LifeBuoy },
  { id: "keys", group: "silo", label: "settings.keys", icon: KeyRound },
  { id: "devices", group: "silo", label: "settings.devices", icon: Laptop },
  { id: "protected", group: "silo", label: "settings.protected", icon: FolderHeart },
  { id: "advanced", group: "silo", label: "settings.advanced", icon: Wrench },
  { id: "general", group: "app", label: "settings.general", icon: SlidersHorizontal },
  { id: "browser", group: "app", label: "settings.browser", icon: Globe },
  { id: "ssh", group: "app", label: "settings.ssh", icon: SquareTerminal },
  { id: "updates", group: "app", label: "settings.updates", icon: DownloadCloud },
] as const;

export type SettingsSectionId = (typeof SECTIONS)[number]["id"];

export function SettingsPanel(props: Props) {
  useLocale();
  const platform = platformStrings(props.os);
  const {
    busy,
    auditLog,
    onAuditChanged,
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
      <ViewHeader
        icon={Settings2}
        title={t("nav.settings")}
        subtitle={t("settings.subtitle", { name: silo.name })}
      />
      <div className="settings-body">
        <nav className="view-rail" aria-label={t("settings.sections")}>
          {SECTIONS.map((item) => {
            const Icon = item.icon;
            const heading = item.group === lastGroup ? null : (lastGroup = item.group);
            return (
              <div key={item.id}>
                {heading && (
                  <div className="view-rail-heading">
                    {heading === "silo" ? silo.name : t("settings.app_heading")}
                  </div>
                )}
                <button
                  type="button"
                  className={`view-rail-item${section === item.id ? " active" : ""}`}
                  aria-current={section === item.id ? "page" : undefined}
                  onClick={() => onSection(item.id)}
                >
                  <Icon size={14} aria-hidden className="settings-rail-icon" />
                  <span className="view-rail-label">{t(item.label)}</span>
                  {item.id === "updates" && updater.state.phase === "available" && (
                    <span className="tab-badge tab-badge-update rail-update-badge">
                      {t("settings.update_badge")}
                    </span>
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
            auditLog={auditLog}
            onGo={(target) =>
              target === "activity" ? props.onOpenActivity() : onSection(target)
            }
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
              {t("settings.recovery")}
            </h3>
            <p>{t("recovery.intro")}</p>
            {recoveryCode ? (
              <div className="recovery-reveal">
                <p className="hint is-error">
                  <AlertTriangle size={14} />
                  {t("recovery.shown_once")}
                </p>
                <code className="recovery-code">{recoveryCode}</code>
                {/* The consequence belongs next to the code itself, where
                    someone is deciding how carefully to store it, rather
                    than only on the setup screen they saw once. */}
                <div className="consequence">
                  <h3>
                    <AlertTriangle size={15} />
                    {t("recovery.only_way_title")}
                  </h3>
                  <p>{t("recovery.only_way_body")}</p>
                </div>
                <div className="actions">
                  <button type="button" onClick={onCopyRecoveryCode}>
                    <Copy size={15} />
                    {t("common.copy")}
                  </button>
                  <button type="button" className="secondary" onClick={onDismissRecoveryCode}>
                    {t("recovery_new.done")}
                  </button>
                </div>
              </div>
            ) : (
              <>
                <p className={`hint${recovery.enabled ? " success-msg" : ""}`}>
                  {recovery.enabled ? (
                    <>
                      <CheckCircle2 size={14} />
                      {recovery.created_at
                        ? t("recovery.active_since", { date: formatDay(recovery.created_at) })
                        : t("recovery.active")}
                    </>
                  ) : (
                    t("recovery.none")
                  )}
                </p>
                <div className="actions">
                  <button type="button" disabled={busy} onClick={onGenerateRecovery}>
                    {recovery.enabled ? t("recovery.replace") : t("first.create_code")}
                  </button>
                </div>
                {recovery.enabled && (
                  <p className="hint">{t("recovery.replace_hint")}</p>
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
                {t("set.rotation_pending")}
              </p>
              <div className="actions">
                <button type="button" disabled={busy} onClick={() => onSection("advanced")}>
                  {t("set.rotation_finish")}
                </button>
              </div>
            </div>
          )}
          <div className="panel-section">
            <h3>
              <KeyRound size={16} />
              {t("set.keys_title")}
            </h3>
            <p>
              {platform.hasBuiltIn
                ? t("set.keys_intro_builtin", { builtin: platform.builtIn })
                : t("set.keys_intro")}
            </p>
            {securityKeys.length === 0 ? (
              <p className="hint empty-state-row">
                <Inbox size={14} />
                {t("set.keys_unreadable")}
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
                        aria-label={
                          k.label
                            ? t("set.key_rename_label", { name: k.label })
                            : t("set.key_rename_label_slot", { slot: k.key_slot })
                        }
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
                          {t("set.save")}
                        </button>
                        <button type="button" className="link" onClick={() => setRenamingKey(null)}>
                          {t("common.cancel")}
                        </button>
                      </div>
                    </li>
                  ) : (
                  <li
                    key={k.credential_id}
                    className="key-list-item"
                    // For telling two keys apart when asking for help; not
                    // something to read on every visit.
                    title={t("set.key_tooltip", {
                      slot: k.key_slot,
                      id: k.credential_id.slice(0, 12),
                    })}
                  >
                    <div>
                      <strong>{securityKeyDisplayName(k, props.os)}</strong>
                      {/* Never hidden, on any device. A key the person at this
                          computer cannot remove is something they are entitled
                          to see named for what it is. */}
                      {k.policy === "org" && (
                        <span className="key-badge" title={t("set.key_org_badge_tooltip")}>
                          <Building2 size={12} aria-hidden />
                          {t("set.key_org_badge")}
                        </span>
                      )}
                      <span className="hint">
                        {" "}
                        ·{" "}
                        {!usableHere(k)
                          ? t("set.key_where_other")
                          : k.platform
                            ? t("set.key_where_local")
                            : t("set.key_where_portable")}
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
                        {t("set.rename")}
                      </button>
                      <button
                        type="button"
                        className="link"
                        disabled={busy || securityKeys.length <= 1}
                        title={
                          securityKeys.length <= 1
                            ? t("set.key_remove_last")
                            : k.policy === "org"
                              ? t("set.key_remove_org")
                              : undefined
                        }
                        onClick={() => onRemoveKey(k.credential_id)}
                      >
                        {t("set.remove")}
                      </button>
                    </div>
                  </li>
                  )
                )}
              </ul>
            )}
            <div className="key-add-panel">
              <p className="hint">{t("set.add_intro")}</p>
              <ol className="hint key-add-steps">
                <li>{t("set.add_step_plug")}</li>
                <li>{t("set.add_step_label")}</li>
                <li>{t("set.add_step_click", { os: platform.osName })}</li>
              </ol>
              <p className="hint">{t("set.add_wait")}</p>
              {!hasPortableKey && !recovery.enabled && securityKeys.length > 0 && (
                <p className="hint is-error">
                  <AlertTriangle size={14} />
                  {t("set.add_only_local")}
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
                    {t("set.org_enrol")}
                    <span className="hint">
                      {t("set.org_enrol_hint")}
                      {platform.hasBuiltIn &&
                        ` ${t("set.org_enrol_hint_builtin", { builtin: platform.builtIn })}`}
                    </span>
                  </span>
                </label>
              )}
              <div className="inline-form explorer-new-folder">
                <input
                  type="text"
                  placeholder={t("set.label_placeholder")}
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
                  {busy && fidoProgress ? t("unlock.waiting") : t("set.add_security_key")}
                </button>
                {platformAvailable && !addAsOrganisation && (
                  <button
                    type="button"
                    className="secondary"
                    disabled={busy}
                    onClick={() => onAddKey("this-device", false)}
                  >
                    {t("set.add_builtin", { builtin: platform.builtIn })}
                  </button>
                )}
              </div>
            </div>
          </div>
          <div className="panel-section">
            <h3>
              <Timer size={16} />
              {t("set.auto_lock_title")}
            </h3>
            <SettingList>
            <SettingRow
              label={tx("set.auto_lock_silo", { name: <strong>{silo.name}</strong> })}
              htmlFor="auto-lock-silo"
              hint={t("set.auto_lock_silo_hint")}
            >
              <select
                id="auto-lock-silo"
                value={siloAutoLockMinutes ?? "default"}
                disabled={busy}
                onChange={(e) =>
                  onSiloAutoLockMinutes(
                    e.target.value === "default" ? null : Number.parseInt(e.target.value, 10),
                  )
                }
              >
                <option value="default">
                  {t("set.auto_lock_default", { time: formatMinutes(autoLockMinutes) })}
                </option>
                {AUTO_LOCK_OPTIONS_MINUTES.map((minutes) => (
                  <option key={minutes} value={minutes}>
                    {formatMinutes(minutes)}
                  </option>
                ))}
              </select>
            </SettingRow>
            </SettingList>
          </div>
          </>
        )}

        {section === "devices" && (
          <>
          <div className="panel-section">
            <h3>
              <Laptop size={16} />
              {t("settings.devices")}
            </h3>
            <p>{t("set.devices_intro")}</p>
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
                      placeholder={device.system_name ?? t("set.device_name_placeholder")}
                      aria-label={t("set.device_rename_label", { id: device.id.slice(0, 8) })}
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
                        {t("set.save")}
                      </button>
                      <button
                        type="button"
                        className="link"
                        onClick={() => setRenamingDevice(null)}
                      >
                        {t("common.cancel")}
                      </button>
                    </div>
                  </li>
                ) : (
                  <li key={device.id} className="key-list-item">
                    <div>
                      <strong>
                        {device.label || device.system_name || t("set.device_unnamed")}
                      </strong>
                      <span className="hint">
                        {device.is_this_device ? ` · ${t("set.device_this")}` : ""}
                        {device.platform ? ` · ${device.platform}` : ""}
                        {/* Only when a person renamed it: otherwise the
                            heading already is the computer name and this
                            would print it twice. */}
                        {device.label && device.system_name ? ` · ${device.system_name}` : ""}
                        {" · "}
                        {t("set.device_changes", { count: device.operations })}
                        {device.last_change_at > 0
                          ? ` · ${t("set.device_last", { date: formatDate(device.last_change_at) })}`
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
                        {t("set.rename")}
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
              {t("set.devices_remove_hint")}
            </p>
          </div>
          <AuditLogPanel busy={busy} onChanged={onAuditChanged} />
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
                  {t("recovery.off_title")}
                </h3>
                <p>{t("recovery.off_body")}</p>
                <div className="actions">
                  <button
                    type="button"
                    className="danger"
                    disabled={busy}
                    onClick={onDisableRecovery}
                  >
                    {t("recovery.off_title")}
                  </button>
                </div>
              </div>
            )}

            <div className="panel-section panel-section-danger">
              <h3 className="is-danger">
                <AlertTriangle size={16} />
                {t("set.remove_silo")}
              </h3>
              <p>{tx("set.remove_silo_body", { name: <strong>{silo.name}</strong> })}</p>
              <div className="actions">
                <button type="button" className="danger" disabled={busy} onClick={onForgetSilo}>
                  {t("set.remove_silo")}
                </button>
              </div>
            </div>
          </>
        )}

        {(section === "general" ||
          section === "browser" ||
          section === "ssh" ||
          section === "updates") && (
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
