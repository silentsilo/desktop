import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Globe, HeartPulse, ShieldCheck } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { ViewHeader } from "../../components/ViewHeader";
import type { PasswordEntry } from "../../lib/types";
import { checkPasswords, type PwnedReport } from "../../lib/pwned";
import { subtitleFor, typeOf, TYPE_LABELS } from "../passwords/util";
import { summarise, type HealthFinding, type HealthFix } from "./analysis";
import { canIgnore, fingerprint } from "./ignored";
import { dateLocale, t, useLocale, type Key } from "../../i18n";

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

/** The breach check, as a state the page can be in. */
type BreachState =
  | { kind: "idle" }
  | { kind: "busy" }
  | { kind: "done"; report: PwnedReport }
  | { kind: "unavailable" };

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
  const [breaches, setBreaches] = useState<BreachState>({ kind: "idle" });
  const entryCount = entries.length;

  const toggle = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const runBreachCheck = async () => {
    setBreaches({ kind: "busy" });
    try {
      const report = await checkPasswords(entries, (prefix) =>
        invoke<string>("pwned_range", { prefix }),
      );
      // Every request failing means the service or the network is gone,
      // and "0 exposed" would be the wrong reading of that.
      setBreaches(
        report.checked > 0 && report.unavailable === report.checked
          ? { kind: "unavailable" }
          : { kind: "done", report },
      );
    } catch {
      setBreaches({ kind: "unavailable" });
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
                    key={fingerprint(finding)}
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
              className="secondary health-finding-fix"
              disabled={breaches.kind === "busy" || entryCount === 0}
              onClick={() => void runBreachCheck()}
            >
              {breaches.kind === "busy" ? t("dlg.health_checking") : t("dlg.health_check_now")}
            </button>
          </div>
          <p className="hint">{t("dlg.health_breach_hint")}</p>
          {breaches.kind === "unavailable" && (
            <p className="hint">{t("dlg.health_breach_unavailable")}</p>
          )}
          {breaches.kind === "done" && (
            <BreachResults report={breaches.report} entries={entries} onOpenEntry={onOpenEntry} />
          )}
        </div>
      </div>
    </div>
  );
}

function BreachResults({
  report,
  entries,
  onOpenEntry,
}: {
  report: PwnedReport;
  entries: PasswordEntry[];
  onOpenEntry: (id: string) => void;
}) {
  useLocale();
  const nameOf = (id: string) =>
    entries.find((e) => e.id === id)?.service || t("dlg.untitled");

  if (report.exposures.length === 0) {
    return (
      <p className="hint">
        {t("dlg.health_breach_none", { count: report.checked })}
        {report.unavailable > 0 &&
          ` ${t("dlg.health_breach_partial", { count: report.unavailable })}`}
      </p>
    );
  }

  return (
    <>
      <p className="health-finding-detail">
        {t("dlg.health_breach_found", { count: report.exposures.length })}
      </p>
      <ul className="health-entry-group">
        {report.exposures.flatMap((exposure) =>
          exposure.entryIds.map((id) => (
            <li key={id}>
              <button
                type="button"
                className="health-entry"
                onClick={() => onOpenEntry(id)}
                title={t("dlg.health_open_entry", { name: nameOf(id) })}
              >
                <span className="health-entry-name">{nameOf(id)}</span>
                <span className="health-entry-sub">
                  {t("dlg.health_seen", {
                    count: exposure.count,
                    times: exposure.count.toLocaleString(dateLocale()),
                  })}
                </span>
              </button>
            </li>
          )),
        )}
      </ul>
    </>
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
    <li
      className={`health-finding health-${finding.severity}${onShowAgain ? " is-ignored" : ""}`}
    >
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
              className="secondary health-finding-fix"
              onClick={() => onOpenFix(finding.fix!)}
            >
              {t(FIX_LABELS[finding.fix])}
            </button>
          )}
          {onIgnore && (
            <button
              type="button"
              className="secondary health-finding-fix"
              onClick={onIgnore}
              title={t("dlg.health_ignore_tooltip")}
            >
              {t("dlg.health_ignore")}
            </button>
          )}
          {onShowAgain && (
            <button type="button" className="secondary health-finding-fix" onClick={onShowAgain}>
              {t("dlg.health_show_again")}
            </button>
          )}
        </span>
      </div>

      <p className="health-finding-detail">{finding.detail}</p>

      {open && expandable && (
        <div className="health-finding-body">
          {finding.groups
            ? finding.groups.map((group, i) => (
                <ul key={group[0]?.id ?? i} className="health-entry-group">
                  {group.map((entry) => (
                    <EntryButton key={entry.id} entry={entry} onOpen={onOpenEntry} />
                  ))}
                </ul>
              ))
            : (
              <ul className="health-entry-group">
                {finding.entries.map((entry) => (
                  <EntryButton key={entry.id} entry={entry} onOpen={onOpenEntry} />
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
  onOpen,
}: {
  entry: PasswordEntry;
  onOpen: (id: string) => void;
}) {
  useLocale();
  const subtitle = subtitleFor(entry);
  return (
    <li>
      <button
        type="button"
        className="health-entry"
        onClick={() => onOpen(entry.id)}
        title={t("dlg.health_open_entry", { name: entry.service })}
      >
        <span className="health-entry-name">{entry.service || t("dlg.untitled")}</span>
        {subtitle && <span className="health-entry-sub">{subtitle}</span>}
        <span className="health-entry-kind">{TYPE_LABELS[typeOf(entry)].singular}</span>
      </button>
    </li>
  );
}
