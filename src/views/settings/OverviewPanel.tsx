import { useEffect, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  CloudUpload,
  Copy,
  HardDrive,
  KeyRound,
  LifeBuoy,
  Printer,
  SearchCheck,
} from "lucide-react";
import type { RecoveryStatus, Silo } from "../../lib/types";
import { currentCopies, type BackupTargetView } from "../../lib/copies";
import { formatAge, formatDay } from "../../lib/format";
import { isDue, lastDone } from "../../lib/siloMemory";
import { platformStrings, type Os } from "../../lib/platformStrings";
import type { SyncIndicator } from "../../layout/AppShell";

/** Where a row's action leads. */
export type OverviewTarget = "backup" | "verify" | "recovery" | "keys";

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
        <button type="button" className="secondary" disabled={busy} onClick={onAction}>
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
  onGo,
  onRenameSilo,
  onSwitchSilo,
}: Props) {
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
    <div className="panel-section">
      <h3>
        <HardDrive size={16} />
        {silo.name}
      </h3>
      <p>What keeps this silo safe. Anything marked needs a look.</p>

      <ul className="key-list overview-list">
        {!sync.configured ? (
          <Row
            icon={<CloudUpload size={16} />}
            title="Backup"
            tone="bad"
            action="Set up backup"
            onAction={() => onGo("backup")}
            busy={busy}
          >
            Not backed up. This silo is only on this computer.
          </Row>
        ) : sync.state === "error" ? (
          <Row
            icon={<CloudUpload size={16} />}
            title="Backup"
            tone="bad"
            action="Open Backup"
            onAction={() => onGo("backup")}
            busy={busy}
          >
            Sync failed{sync.lastError ? `: ${sync.lastError}` : "."}
          </Row>
        ) : (
          <Row
            icon={<CloudUpload size={16} />}
            title="Backup"
            tone="ok"
            action="Open Backup"
            onAction={() => onGo("backup")}
            busy={busy}
          >
            {sync.lastSyncAt ? `Synced ${formatAge(sync.lastSyncAt)}.` : "Backup storage connected."}
          </Row>
        )}

        {sync.configured && targets && (
          <Row
            icon={<Copy size={16} />}
            title="Copies"
            tone={current >= 3 ? "ok" : "warn"}
            action={current >= 3 ? "Open" : "Add a copy"}
            onAction={() => onGo("backup")}
            busy={busy}
          >
            {current} of {copies} {copies === 1 ? "copy" : "copies"} up to date.
            {current < 3 ? " Aim for three, with one somewhere else." : ""}
          </Row>
        )}

        <Row
          icon={<LifeBuoy size={16} />}
          title="Recovery code"
          tone={recovery.enabled ? "ok" : "bad"}
          action={recovery.enabled ? "Open" : "Create one"}
          onAction={() => onGo("recovery")}
          busy={busy}
        >
          {recovery.enabled
            ? `Set${recovery.created_at ? `, created ${formatDay(recovery.created_at)}` : ""}.`
            : "None yet. Losing every key would mean losing the silo."}
        </Row>

        {recovery.enabled && (
          <Row
            icon={<Printer size={16} />}
            title="Emergency kit"
            tone={printed ? "ok" : "neutral"}
            action={printed ? "Print again" : "Print it"}
            onAction={() => onGo("recovery")}
            busy={busy}
          >
            {/* The app never keeps the code, so it cannot know about a kit
                printed elsewhere or a code written on other paper: this is
                a suggestion, not a problem. */}
            {printed
              ? `Printed ${formatDay(Math.floor(printed / 1000))} on this computer.`
              : "No kit printed on this computer yet. It is one sheet with the steps to get back in."}
          </Row>
        )}

        <Row
          icon={<KeyRound size={16} />}
          title="A key you can carry"
          tone={hasPortableKey ? "ok" : "warn"}
          action={hasPortableKey ? "Open" : "Add a key"}
          onAction={() => onGo("keys")}
          busy={busy}
        >
          {hasPortableKey
            ? "A security key opens this silo from any computer."
            : `Only ${platform.builtIn} opens it, and that works only on this computer.`}
        </Row>

        {sync.configured && (
          <Row
            icon={<SearchCheck size={16} />}
            title="Last test"
            tone={isDue(tested, REMIND_AFTER_DAYS) ? "warn" : "ok"}
            action="Test backup"
            onAction={() => onGo("verify")}
            busy={busy}
          >
            {tested
              ? `Tested ${formatDay(Math.floor(tested / 1000))} on this computer.`
              : "Never tested from this computer."}
          </Row>
        )}
      </ul>

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
        <button type="button" className="secondary" disabled={busy} onClick={onSwitchSilo}>
          Switch silo
        </button>
      </div>
      <p className="hint">
        Renaming changes the label only. The folder keeps its name, so backups and shortcuts
        pointing at it keep working.
      </p>
    </div>
  );
}
