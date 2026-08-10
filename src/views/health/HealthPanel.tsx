import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, Globe, HeartPulse, ShieldCheck } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { ViewHeader } from "../../components/ViewHeader";
import type { PasswordEntry } from "../../lib/types";
import { checkPasswords, type PwnedReport } from "../../lib/pwned";
import { subtitleFor, typeOf, TYPE_LABELS } from "../passwords/util";
import { summarise, type HealthFinding, type HealthFix } from "./analysis";

type Props = {
  /** Computed by the shell, which also badges the count on the tab: one
   * analysis, so the number on the tab and the list here cannot disagree. */
  findings: HealthFinding[];
  /** The credentials themselves, for the on-demand breach check. */
  entries: PasswordEntry[];
  /** Opens Credentials on that entry, filters cleared. */
  onOpenEntry: (id: string) => void;
  /** Opens the Settings section where a silo-level finding is fixed. */
  onOpenFix: (fix: HealthFix) => void;
};

const FIX_LABELS: Record<HealthFix, string> = {
  backup: "Set up backup",
  keys: "Add a security key",
  recovery: "Create a recovery code",
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
export function HealthPanel({ findings, entries, onOpenEntry, onOpenFix }: Props) {
  const counts = useMemo(() => summarise(findings), [findings]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
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
      ? `Nothing to fix across ${entryCount} ${entryCount === 1 ? "entry" : "entries"}`
      : `${findings.length} ${findings.length === 1 ? "thing" : "things"} to look at across ` +
        `${entryCount} ${entryCount === 1 ? "entry" : "entries"}`;

  return (
    <div className="health-view">
      <ViewHeader icon={HeartPulse} title="Health" subtitle={subtitle} />

      {findings.length > 0 && (
        <div className="health-counters">
          <span className="health-counter health-high">
            {counts.high} to fix
          </span>
          <span className="health-counter health-medium">
            {counts.medium} worth doing
          </span>
          <span className="health-counter health-info">{counts.info} to know</span>
        </div>
      )}

      <div className="health-pane">
        {findings.length === 0 ? (
          <div className="health-empty-state">
            <ShieldCheck size={28} />
            <p className="hint">
              No reused or weak passwords, and this silo has a recovery code, a spare key and a
              backup.
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
              />
            ))}
          </ul>
        )}

        <div className="health-breach">
          <div className="health-breach-head">
            <Globe size={16} aria-hidden />
            <span className="health-finding-title">Passwords in known breaches</span>
            <button
              type="button"
              className="secondary health-finding-fix"
              disabled={breaches.kind === "busy" || entryCount === 0}
              onClick={() => void runBreachCheck()}
            >
              {breaches.kind === "busy" ? "Checking…" : "Check now"}
            </button>
          </div>
          <p className="hint">
            Compares your passwords against the Have I Been Pwned corpus. Only the first five
            characters of each password's hash leave this computer; the passwords themselves
            never do, and nothing in this silo is changed by the check.
          </p>
          {breaches.kind === "unavailable" && (
            <p className="hint">
              The breach service could not be reached. Nothing was checked; try again later.
            </p>
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
  const nameOf = (id: string) => entries.find((e) => e.id === id)?.service || "Untitled";

  if (report.exposures.length === 0) {
    return (
      <p className="hint">
        None of the {report.checked} distinct {report.checked === 1 ? "password" : "passwords"}{" "}
        checked appears in known breaches.
        {report.unavailable > 0 &&
          ` ${report.unavailable} could not be checked because the service did not answer.`}
      </p>
    );
  }

  return (
    <>
      <p className="health-finding-detail">
        {report.exposures.length} {report.exposures.length === 1 ? "password appears" : "passwords appear"}{" "}
        in breached data. Change each on the site first, then here.
      </p>
      <ul className="health-entry-group">
        {report.exposures.flatMap((exposure) =>
          exposure.entryIds.map((id) => (
            <li key={id}>
              <button
                type="button"
                className="health-entry"
                onClick={() => onOpenEntry(id)}
                title={`Open ${nameOf(id)} in Credentials`}
              >
                <span className="health-entry-name">{nameOf(id)}</span>
                <span className="health-entry-sub">
                  seen {exposure.count.toLocaleString("en-US")} times
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
}: {
  finding: HealthFinding;
  open: boolean;
  onToggle: () => void;
  onOpenEntry: (id: string) => void;
  onOpenFix: (fix: HealthFix) => void;
}) {
  const expandable = finding.entries.length > 0;

  return (
    <li className={`health-finding health-${finding.severity}`}>
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

        {finding.fix && (
          <button
            type="button"
            className="secondary health-finding-fix"
            onClick={() => onOpenFix(finding.fix!)}
          >
            {FIX_LABELS[finding.fix]}
          </button>
        )}
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
  const subtitle = subtitleFor(entry);
  return (
    <li>
      <button
        type="button"
        className="health-entry"
        onClick={() => onOpen(entry.id)}
        title={`Open ${entry.service} in Credentials`}
      >
        <span className="health-entry-name">{entry.service || "Untitled"}</span>
        {subtitle && <span className="health-entry-sub">{subtitle}</span>}
        <span className="health-entry-kind">{TYPE_LABELS[typeOf(entry)].singular}</span>
      </button>
    </li>
  );
}
