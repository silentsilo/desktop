import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Globe, HeartPulse, ShieldCheck } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { ViewHeader } from "../../components/ViewHeader";
import type { PasswordEntry } from "../../lib/types";
import { checkPasswords, type PwnedReport } from "../../lib/pwned";
import { subtitleFor, typeOf, TYPE_LABELS } from "../passwords/util";
import { summarise, type HealthFinding, type HealthFix } from "./analysis";
import { canIgnore, findingKey } from "./ignored";
import { dateLocale, t, useLocale, type Key } from "../../i18n";
import { useLasting, useLastingCheck } from "../../lib/lasting";

type Props = {
  /** Computed by the shell, which also badges the count on the tab: one
   * analysis, so the number on the tab and the list here cannot disagree. */
  findings: HealthFinding[];
  /** Findings set aside on this computer: listed apart and left out of
   * every count. */
  ignored: HealthFinding[];
  onIgnore: (finding: HealthFinding) => void;
  onShowAgain: (finding: HealthFinding) => void;
  /** The credentials themselves, for the on-demand breach check. */
  entries: PasswordEntry[];
  /** Opens Credentials on that entry, filters cleared. */
  onOpenEntry: (id: string) => void;
  /** Opens the Settings section where a silo-level finding is fixed. */
  onOpenFix: (fix: HealthFix) => void;
};

const FIX_LABELS: Record<HealthFix, Key> = {
  backup: "dlg.health_fix_backup",
  keys: "dlg.health_fix_keys",
  recovery: "dlg.health_fix_recovery",
  verify: "settings.verify",
};

/** The breach check, as a state the page can be in. `stamps` holds each
 * entry's `updated_at` at the check, so an entry changed since is not
 * reported on a password it no longer has. */
export type BreachState =
  | { kind: "idle" }
  | { kind: "busy"; done?: number; total?: number }
  | { kind: "done"; report: PwnedReport; stamps: Record<string, number> }
  | { kind: "unavailable" };

/** Where the check's result is kept until the silo locks; the shell reads
 * it too, so a breached password counts like any other finding. */
export const BREACHES_KEY = "health.breaches";

/** The entries a finished check still holds against: id to times seen. */
export function breachedEntries(
  state: BreachState,
  entries: PasswordEntry[],
): ReadonlyMap<string, number> {
  const found = new Map<string, number>();
  if (state.kind !== "done") return found;
  const now = new Map(entries.map((e) => [e.id, e.updated_at]));
  for (const exposure of state.report.exposures) {
    for (const id of exposure.entryIds) {
      if (now.get(id) === state.stamps[id]) found.set(id, exposure.count);
    }
  }
  return found;
}

/**
 * What is wrong with this silo, in one place.
 *
 * Everything that runs by itself is computed locally from entries already
 * in memory: opening this page sends nothing anywhere. The one exception is
 * the breach check below, which runs only when its button is pressed and
 * sends five characters of a hash per distinct password (k-anonymity). A
 * vault that promises no server does not open connections to grade its own
 * contents unasked; asked is different.
 *
 * Findings are collapsed by default. The count is the part that decides
 * whether to look; the list of entries is the part you act on, one at a
 * time, and expanded lists would bury the next finding under the first.
 */
export const BREACHES_IDLE: BreachState = { kind: "idle" };

export function HealthPanel({
  findings,
  ignored,
  onIgnore,
  onShowAgain,
  entries,
  onOpenEntry,
  onOpenFix,
}: Props) {
  useLocale();
  const counts = useMemo(() => summarise(findings), [findings]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [showIgnored, setShowIgnored] = useState(false);
  // Lasting until the silo locks: the check asks the network about every
  // password, and its answer should not vanish on leaving the page.
  const startedFor = useLastingCheck();
  const [breaches, setBreaches] = useLasting<BreachState>(
    BREACHES_KEY,
    BREACHES_IDLE,
    (b) => b.kind !== "idle",
  );
  const entryCount = entries.length;
  // Entries no finding names: the one number that says how the silo is doing.
  const fineCount = useMemo(() => {
    const named = new Set<string>();
    for (const finding of findings) for (const entry of finding.entries) named.add(entry.id);
    return entries.filter((entry) => !named.has(entry.id)).length;
  }, [findings, entries]);

  const toggle = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const runBreachCheck = async () => {
    setBreaches({ kind: "busy" });
    // A lock while it runs stops it, and its answer is kept nowhere.
    const current = startedFor();
    const stamps = Object.fromEntries(entries.map((e) => [e.id, e.updated_at]));
    try {
      const report = await checkPasswords(
        entries,
        (prefix) => invoke<string>("pwned_range", { prefix }),
        (done, total) => {
          if (current()) setBreaches({ kind: "busy", done, total });
        },
        () => !current(),
      );
      if (!current()) return;
      // Every request failing means the service or the network is gone,
      // and "0 exposed" would be the wrong reading of that.
      setBreaches(
        report.checked > 0 && report.unavailable === report.checked
          ? { kind: "unavailable" }
          : { kind: "done", report, stamps },
      );
    } catch {
      if (current()) setBreaches({ kind: "unavailable" });
    }
  };

  const subtitle =
    findings.length === 0
      ? t("dlg.health_nothing", { count: entryCount })
      : t("dlg.health_things", {
          count: findings.length,
          entries: t("dlg.health_entry_count", { count: entryCount }),
        });

  return (
    <div className="health-view">
      <ViewHeader icon={HeartPulse} title={t("nav.health")} subtitle={subtitle} />

      {findings.length > 0 && (
        <div className="health-summary">
          {entryCount > 0 && (
            <div className="health-score">
              <p className="health-score-number">
                <strong>{fineCount}</strong> / {entryCount}
              </p>
              <p className="health-score-label">{t("dlg.health_score_label")}</p>
            </div>
          )}
          <div className="health-counters">
            <span className="health-counter health-high">
              {t("dlg.health_to_fix", { count: counts.high })}
            </span>
            <span className="health-counter health-medium">
              {t("dlg.health_worth_doing", { count: counts.medium })}
            </span>
            <span className="health-counter health-info">
              {t("dlg.health_to_know", { count: counts.info })}
            </span>
          </div>
        </div>
      )}

      <div className="health-pane">
        {findings.length === 0 ? (
          <div className="health-empty-state">
            <ShieldCheck size={28} />
            <p className="hint">
              {ignored.length > 0 ? t("dlg.health_nothing_else") : t("dlg.health_all_good")}
            </p>
          </div>
        ) : (
          <ul className="health-list">
            {findings.map((finding) => (
              <FindingRow
                key={finding.id}
                finding={finding}
                open={expanded.has(finding.id)}
                onToggle={() => toggle(finding.id)}
                onOpenEntry={onOpenEntry}
                onOpenFix={onOpenFix}
                onIgnore={canIgnore(finding) ? () => onIgnore(finding) : undefined}
              />
            ))}
          </ul>
        )}

        {ignored.length > 0 && (
          <div className="health-ignored">
            <button
              type="button"
              className="health-ignored-toggle"
              onClick={() => setShowIgnored((v) => !v)}
              aria-expanded={showIgnored}
            >
              {showIgnored ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
              {t("dlg.health_ignored", { count: ignored.length })}
            </button>
            {showIgnored && (
              <ul className="health-list">
                {ignored.map((finding) => (
                  <FindingRow
                    key={findingKey(finding)}
                    finding={finding}
                    open={expanded.has(finding.id)}
                    onToggle={() => toggle(finding.id)}
                    onOpenEntry={onOpenEntry}
                    onOpenFix={onOpenFix}
                    onShowAgain={() => onShowAgain(finding)}
                  />
                ))}
              </ul>
            )}
          </div>
        )}

        <div className="health-breach">
          <div className="health-breach-head">
            <Globe size={16} aria-hidden />
            <span className="health-finding-title">{t("dlg.health_breach_title")}</span>
            <button
              type="button"
              className="btn-secondary btn-sm health-finding-fix"
              disabled={breaches.kind === "busy" || entryCount === 0}
              onClick={() => void runBreachCheck()}
            >
              {breaches.kind === "busy"
                ? breaches.total
                  ? t("dlg.health_checking_count", {
                      done: breaches.done ?? 0,
                      total: breaches.total,
                    })
                  : t("dlg.health_checking")
                : t("dlg.health_check_now")}
            </button>
          </div>
          <p className="hint">{t("dlg.health_breach_hint")}</p>
          {breaches.kind === "unavailable" && (
            <p className="hint">{t("dlg.health_breach_unavailable")}</p>
          )}
          {breaches.kind === "done" && <BreachResults report={breaches.report} />}
        </div>
      </div>
    </div>
  );
}

/** The check's outcome in one line; what it found is listed as a finding
 * above, with the other things to fix. */
function BreachResults({ report }: { report: PwnedReport }) {
  useLocale();
  if (report.exposures.length === 0) {
    return (
      <p className="hint">
        {t("dlg.health_breach_none", { count: report.checked - report.unavailable })}
        {report.unavailable > 0 &&
          ` ${t("dlg.health_breach_partial", { count: report.unavailable })}`}
      </p>
    );
  }

  return (
    <p className="health-finding-detail">
      {t("dlg.health_breach_found", { count: report.exposures.length })}
    </p>
  );
}

function FindingRow({
  finding,
  open,
  onToggle,
  onOpenEntry,
  onOpenFix,
  onIgnore,
  onShowAgain,
}: {
  finding: HealthFinding;
  open: boolean;
  onToggle: () => void;
  onOpenEntry: (id: string) => void;
  onOpenFix: (fix: HealthFix) => void;
  /** Only for what is not critical. */
  onIgnore?: () => void;
  /** Only on an ignored finding. */
  onShowAgain?: () => void;
}) {
  useLocale();
  const expandable = finding.entries.length > 0;

  return (
    <li className={`health-finding health-${finding.severity}${onShowAgain ? " is-ignored" : ""}`}>
      <div className="health-finding-head">
        {expandable ? (
          <button
            type="button"
            className="health-finding-toggle"
            onClick={onToggle}
            aria-expanded={open}
          >
            {open ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
            <span className="health-finding-title">{finding.title}</span>
          </button>
        ) : (
          <span className="health-finding-title health-finding-title-static">{finding.title}</span>
        )}

        <span className="health-finding-actions">
          {finding.fix && !onShowAgain && (
            <button
              type="button"
              className="btn-secondary btn-sm health-finding-fix"
              onClick={() => onOpenFix(finding.fix!)}
            >
              {t(FIX_LABELS[finding.fix])}
            </button>
          )}
          {onIgnore && (
            <button
              type="button"
              className="btn-secondary btn-sm health-finding-fix"
              onClick={onIgnore}
              data-tooltip={t("dlg.health_ignore_tooltip")}
            >
              {t("dlg.health_ignore")}
            </button>
          )}
          {onShowAgain && (
            <button
              type="button"
              className="btn-secondary btn-sm health-finding-fix"
              onClick={onShowAgain}
            >
              {t("dlg.health_show_again")}
            </button>
          )}
        </span>
      </div>

      <p className="health-finding-detail">{finding.detail}</p>

      {open && expandable && (
        <div className="health-finding-body">
          {finding.groups ? (
            finding.groups.map((group, i) => (
              <ul key={group[0]?.id ?? i} className="health-entry-group">
                {group.map((entry) => (
                  <EntryButton
                    key={entry.id}
                    entry={entry}
                    seen={finding.seen?.get(entry.id)}
                    onOpen={onOpenEntry}
                  />
                ))}
              </ul>
            ))
          ) : (
            <ul className="health-entry-group">
              {finding.entries.map((entry) => (
                <EntryButton
                  key={entry.id}
                  entry={entry}
                  seen={finding.seen?.get(entry.id)}
                  onOpen={onOpenEntry}
                />
              ))}
            </ul>
          )}
        </div>
      )}
    </li>
  );
}

function EntryButton({
  entry,
  seen,
  onOpen,
}: {
  entry: PasswordEntry;
  /** Times its password was seen in breaches, shown instead of the subtitle. */
  seen?: number;
  onOpen: (id: string) => void;
}) {
  useLocale();
  const subtitle =
    seen !== undefined
      ? t("dlg.health_seen", { count: seen, times: seen.toLocaleString(dateLocale()) })
      : subtitleFor(entry);
  return (
    <li>
      <button
        type="button"
        className="health-entry"
        onClick={() => onOpen(entry.id)}
        data-tooltip={t("dlg.health_open_entry", { name: entry.service })}
      >
        <span className="health-entry-name">{entry.service || t("dlg.untitled")}</span>
        {subtitle && <span className="health-entry-sub">{subtitle}</span>}
        <span className="health-entry-kind">{TYPE_LABELS[typeOf(entry)].singular}</span>
      </button>
    </li>
  );
}
