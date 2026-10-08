import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CloudUpload,
  Copy,
  FolderOpen,
  HardDrive,
  KeyRound,
  LifeBuoy,
  Printer,
  ScrollText,
  SearchCheck,
} from "lucide-react";
import type { AuditStatus, RecoveryStatus, Silo } from "../../lib/types";
import { currentCopies, type BackupTargetView } from "../../lib/copies";
import { formatAge, formatDay } from "../../lib/format";
import { isDue, lastDone } from "../../lib/siloMemory";
import { platformStrings, type Os } from "../../lib/platformStrings";
import type { SyncIndicator } from "../../layout/AppShell";
import { t, useLocale } from "../../i18n";
import { SettingList, SettingRow } from "../../components/Setting";

/** Where a row's action leads. */
export type OverviewTarget = "backup" | "verify" | "recovery" | "keys" | "devices" | "activity";

/** How long a test or a printed kit counts as recent. */
const REMIND_AFTER_DAYS = 90;

type Tone = "ok" | "warn" | "bad" | "neutral";

/** The copies list's colours, which say the same three things; neutral is
 * for a suggestion that is not a problem. */
const TONE_CLASS: Record<Tone, string> = {
  ok: "current",
  warn: "behind",
  bad: "stale",
  neutral: "neutral",
};

function Row({
  icon,
  title,
  tone,
  children,
  action,
  onAction,
  busy,
}: {
  icon: ReactNode;
  title: string;
  tone: Tone;
  children: ReactNode;
  action: string;
  onAction: () => void;
  busy: boolean;
}) {
  return (
    <li className="key-list-item overview-row">
      <span className={`copy-icon is-${TONE_CLASS[tone]}`} aria-hidden>
        {icon}
      </span>
      <div className="protected-row-text">
        <strong>{title}</strong>
        <span className={`hint copy-state is-${TONE_CLASS[tone]}`}>{children}</span>
      </div>
      <div className="key-list-actions">
        <button type="button" className="btn-secondary" disabled={busy} onClick={onAction}>
          {action}
        </button>
      </div>
    </li>
  );
}

type Props = {
  os: Os;
  busy: boolean;
  silo: Silo;
  sync: SyncIndicator;
  recovery: RecoveryStatus;
  hasPortableKey: boolean;
  /** Whether this computer holds every file, so it counts as a copy. */
  fullCopy: boolean;
  auditLog: AuditStatus | null;
  onGo: (target: OverviewTarget) => void;
  onRenameSilo: (name: string) => void;
  onSwitchSilo: () => void;
};

/**
 * Where Settings opens: what keeps this silo safe, one line each, and the
 * action that fixes what is missing.
 */
export function OverviewPanel({
  os,
  busy,
  silo,
  sync,
  recovery,
  hasPortableKey,
  fullCopy,
  auditLog,
  onGo,
  onRenameSilo,
  onSwitchSilo,
}: Props) {
  useLocale();
  const platform = platformStrings(os);
  const [siloName, setSiloName] = useState(silo.name);
  const [targets, setTargets] = useState<BackupTargetView[] | null>(null);

  useEffect(() => {
    if (!sync.configured) {
      setTargets(null);
      return;
    }
    let cancelled = false;
    void invoke<BackupTargetView[]>("backup_targets_list")
      .then((list) => {
        if (!cancelled) setTargets(list);
      })
      .catch(() => {
        if (!cancelled) setTargets(null);
      });
    return () => {
      cancelled = true;
    };
  }, [sync.configured, sync.lastSyncAt]);

  const tested = Math.max(lastDone(silo.id, "verified") ?? 0, lastDone(silo.id, "restore-tested") ?? 0) || null;
  const printed = lastDone(silo.id, "kit-printed");

  const now = Math.floor(Date.now() / 1000);
  const copies = targets ? targets.length + (fullCopy ? 1 : 0) : 0;
  const current = targets ? currentCopies(targets, now) + (fullCopy ? 1 : 0) : 0;

  return (
    <>
    <div className="panel-section">
      <h3>
        <HardDrive size={16} />
        {silo.name}
      </h3>
      <p>{t("set.ov_intro")}</p>

      <ul className="key-list overview-list">
        {!sync.configured ? (
          <Row
            icon={<CloudUpload size={16} />}
            title={t("settings.backup")}
            tone="bad"
            action={t("set.ov_setup_backup")}
            onAction={() => onGo("backup")}
            busy={busy}
          >
            {t("set.ov_not_backed_up")}
          </Row>
        ) : sync.state === "error" ? (
          <Row
            icon={<CloudUpload size={16} />}
            title={t("settings.backup")}
            tone="bad"
            action={t("set.ov_open_backup")}
            onAction={() => onGo("backup")}
            busy={busy}
          >
            {sync.lastError
              ? t("set.ov_sync_failed_reason", { reason: sync.lastError })
              : t("set.ov_sync_failed")}
          </Row>
        ) : (
          <Row
            icon={<CloudUpload size={16} />}
            title={t("settings.backup")}
            tone="ok"
            action={t("set.ov_open_backup")}
            onAction={() => onGo("backup")}
            busy={busy}
          >
            {sync.lastSyncAt
              ? t("set.ov_synced", { age: formatAge(sync.lastSyncAt) })
              : t("set.ov_storage_connected")}
          </Row>
        )}

        {sync.configured && targets && (
          <Row
            icon={<Copy size={16} />}
            title={t("set.ov_copies")}
            tone={current >= 3 ? "ok" : "warn"}
            action={current >= 3 ? t("set.ov_open") : t("set.ov_add_copy")}
            onAction={() => onGo("backup")}
            busy={busy}
          >
            {t("set.ov_copies_current", { current, count: copies })}
            {current < 3 ? ` ${t("set.ov_copies_aim")}` : ""}
          </Row>
        )}

        <Row
          icon={<LifeBuoy size={16} />}
          title={t("settings.recovery")}
          tone={recovery.enabled ? "ok" : "bad"}
          action={recovery.enabled ? t("set.ov_open") : t("set.ov_create_one")}
          onAction={() => onGo("recovery")}
          busy={busy}
        >
          {recovery.enabled
            ? recovery.created_at
              ? t("set.ov_recovery_set_on", { date: formatDay(recovery.created_at) })
              : t("set.ov_recovery_set")
            : t("set.ov_recovery_none")}
        </Row>

        {recovery.enabled && (
          <Row
            icon={<Printer size={16} />}
            title={t("set.ov_kit")}
            tone={printed ? "ok" : "neutral"}
            action={printed ? t("set.ov_print_again") : t("set.ov_print")}
            onAction={() => onGo("recovery")}
            busy={busy}
          >
            {/* The app never keeps the code, so it cannot know about a kit
                printed elsewhere or a code written on other paper: this is
                a suggestion, not a problem. */}
            {printed
              ? t("set.ov_kit_printed", { date: formatDay(Math.floor(printed / 1000)) })
              : t("set.ov_kit_none")}
          </Row>
        )}

        <Row
          icon={<KeyRound size={16} />}
          title={t("set.ov_portable")}
          tone={hasPortableKey ? "ok" : "warn"}
          action={hasPortableKey ? t("set.ov_open") : t("set.ov_add_key")}
          onAction={() => onGo("keys")}
          busy={busy}
        >
          {hasPortableKey
            ? t("set.ov_portable_yes")
            : t("set.ov_portable_no", { builtin: platform.builtIn })}
        </Row>

        {sync.configured && (
          <Row
            icon={<SearchCheck size={16} />}
            title={t("set.ov_last_test")}
            tone={isDue(tested, REMIND_AFTER_DAYS) ? "warn" : "ok"}
            action={t("settings.verify")}
            onAction={() => onGo("verify")}
            busy={busy}
          >
            {tested
              ? t("set.ov_tested", { date: formatDay(Math.floor(tested / 1000)) })
              : t("set.ov_never_tested")}
          </Row>
        )}
        <Row
          icon={<ScrollText size={16} />}
          title={t("nav.activity")}
          tone="neutral"
          action={auditLog?.enabled || auditLog?.organisation ? t("set.ov_open") : t("set.ov_turn_on")}
          onAction={() => onGo(auditLog?.enabled || auditLog?.organisation ? "activity" : "devices")}
          busy={busy}
        >
          {auditLog?.organisation
            ? t("set.ov_activity_org")
            : auditLog?.enabled
              ? t("set.ov_activity_on")
              : t("set.ov_activity_off")}
        </Row>
      </ul>
    </div>

    <div className="panel-section">
      <h3>
        <FolderOpen size={16} />
        {t("set.ov_card_silo")}
      </h3>
      <SettingList>
        <SettingRow
          label={t("set.ov_name_label")}
          htmlFor="silo-name"
          hint={t("set.ov_rename_hint")}
        >
          <form
            className="setting-inline"
            onSubmit={(e) => {
              e.preventDefault();
              if (siloName.trim() && siloName.trim() !== silo.name) onRenameSilo(siloName.trim());
            }}
          >
            <input
              id="silo-name"
              type="text"
              value={siloName}
              disabled={busy}
              onChange={(e) => setSiloName(e.target.value)}
              aria-label={t("set.ov_silo_name")}
            />
            <button
              type="submit"
              className="btn-secondary"
              disabled={busy || siloName.trim() === silo.name || !siloName.trim()}
            >
              {t("set.rename")}
            </button>
          </form>
        </SettingRow>
        <SettingRow
          label={t("set.ov_folder_label")}
          hint={<code className="setting-path">{silo.path}</code>}
        >
          {null}
        </SettingRow>
        <SettingRow label={t("set.ov_other_label")} hint={t("set.ov_other_hint")}>
          <button type="button" className="btn-secondary" disabled={busy} onClick={onSwitchSilo}>
            {t("set.ov_switch")}
          </button>
        </SettingRow>
      </SettingList>
    </div>
    </>
  );
}
