import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, HeartPulse, ShieldCheck } from "lucide-react";
import { ViewHeader } from "../../components/ViewHeader";
import type { PasswordEntry } from "../../lib/types";
import { subtitleFor, typeOf, TYPE_LABELS } from "../passwords/util";
import { summarise, type HealthFinding, type HealthFix } from "./analysis";

type Props = {
  /** Computed by the shell, which also badges the count on the tab: one
   * analysis, so the number on the tab and the list here cannot disagree. */
  findings: HealthFinding[];
  /** How many credentials were examined, for the line under the title. */
  entryCount: number;
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

/**
 * What is wrong with this silo, in one place.
 *
 * Everything on this page is computed locally from entries already in
 * memory: no hashes are sent anywhere, and there is no breach lookup. The
 * findings that change behaviour (one password on twelve sites, a silo with
 * no way back in) need no outside help, and a vault that promises no server
 * should not open a connection to grade its own contents.
 *
 * Findings are collapsed by default. The count is the part that decides
 * whether to look; the list of entries is the part you act on, one at a
 * time, and expanded lists would bury the next finding under the first.
 */
export function HealthPanel({ findings, entryCount, onOpenEntry, onOpenFix }: Props) {
  const counts = useMemo(() => summarise(findings), [findings]);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  const toggle = (id: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

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
      </div>
    </div>
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
