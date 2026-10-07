import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AppWindow,
  CirclePlus,
  Copy,
  Dot,
  Download,
  ExternalLink,
  Eye,
  FileDown,
  FilePlus,
  FileText,
  FileUp,
  Globe,
  History,
  KeyRound,
  Laptop,
  LifeBuoy,
  Lock,
  LockOpen,
  Pencil,
  RefreshCw,
  ScrollText,
  ShieldAlert,
  SquareTerminal,
  Timer,
  Trash,
  Trash2,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import { save as saveFileDialog } from "../lib/dialog";
import { formatAppError } from "../lib/errors";
import {
  byDay,
  describe,
  KIND_CODES,
  KIND_LABELS,
  type ActivityIcon,
  type ActivityKind,
} from "../lib/activityEvents";
import { IconSearch } from "../ui/Icons";
import type { AuditEntry, AuditPage } from "../lib/types";

type Props = {
  devices: { id: string; label: string | null; system_name: string | null }[];
  /** An organisation's log: read only when asked, since it takes a key. */
  needsKey?: boolean;
};

/** Rows fetched at a time. */
const PAGE = 100;

const ICONS: Record<ActivityIcon, LucideIcon> = {
  unlock: LockOpen,
  lock: Lock,
  refused: ShieldAlert,
  show: Eye,
  copy: Copy,
  code: Timer,
  browser: Globe,
  app: AppWindow,
  ssh: SquareTerminal,
  create: CirclePlus,
  edit: Pencil,
  delete: Trash2,
  restore: History,
  import: FileDown,
  export: FileUp,
  file: FileText,
  "file-out": ExternalLink,
  "file-add": FilePlus,
  trash: Trash,
  key: KeyRound,
  recovery: LifeBuoy,
  rotate: RefreshCw,
  log: ScrollText,
  device: Laptop,
  repair: Wrench,
  other: Dot,
};

const KINDS = Object.keys(KIND_LABELS) as ActivityKind[];

function timeOf(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * The silo's activity log, read from this computer and every copy, newest
 * first and by day. Says what is missing rather than leaving it out
 * silently: a hole in a device's run is what a reader of a log most needs
 * to see. Searching and filtering run in Rust, over the whole log.
 */
export function AuditLogList({ devices, needsKey = false }: Props) {
  const [log, setLog] = useState<AuditPage | null>(null);
  const [loading, setLoading] = useState(!needsKey);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [kind, setKind] = useState<ActivityKind | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  // Answers for a search typed since are dropped.
  const asked = useRef(0);
  // What the shown page was read with, and what is typed and chosen now:
  // the debounce reads these when it fires, not what it saw when set.
  const [shownFor, setShownFor] = useState<{
    search: string;
    kind: ActivityKind | null;
  }>({
    search: "",
    kind: null,
  });
  const current = useRef({ search, kind });
  current.current = { search, kind };
  const debounce = useRef<number | undefined>(undefined);

  const nameOf = useCallback(
    (id: string) => {
      const device = devices.find((d) => d.id === id);
      return device?.label || device?.system_name || `Device ${id.slice(0, 8)}`;
    },
    [devices],
  );

  /** A page from Rust: the first one after reading every copy again
   * (`refresh`), or the next from the read already held. */
  const fetchPage = useCallback(
    async (
      refresh: boolean,
      offset: number,
      term: string,
      filter: ActivityKind | null,
      append: boolean,
    ) => {
      const ticket = ++asked.current;
      setLoading(true);
      setError(null);
      const ask = (again: boolean, from: number) =>
        invoke<AuditPage>("audit_read", {
          refresh: again,
          offset: from,
          limit: PAGE,
          search: term,
          kinds: filter ? KIND_CODES[filter] : null,
        });
      try {
        let page: AuditPage;
        let adding = append;
        try {
          page = await ask(refresh, offset);
        } catch (e) {
          if (refresh) throw e;
          // The held read went with a lock or a switch. An organisation's
          // takes a key to read again, which needs the person's click: back
          // to the button that asks for it.
          if (needsKey) {
            if (ticket === asked.current) setLog(null);
            throw e;
          }
          // Read again from the top: events may have come in since, and
          // what is shown would no longer line up with the new offsets.
          page = await ask(true, 0);
          adding = false;
        }
        if (ticket !== asked.current) return;
        setLog((prev) =>
          adding && prev ? { ...page, entries: [...prev.entries, ...page.entries] } : page,
        );
        setShownFor({ search: term, kind: filter });
      } catch (e) {
        if (ticket === asked.current) setError(formatAppError(e));
      } finally {
        if (ticket === asked.current) setLoading(false);
      }
    },
    [needsKey],
  );

  const load = useCallback(
    () => fetchPage(true, 0, search, kind, false),
    [fetchPage, search, kind],
  );

  useEffect(() => {
    if (!needsKey) void fetchPage(true, 0, "", null, false);
    // What was read stays in Rust only while the page is open.
    return () => {
      void invoke("audit_read_close").catch(() => {});
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [needsKey]);

  // A search runs in Rust over the whole log, a moment after the typing
  // stops; a filter at once. Either one made while the first read was
  // still running is applied when it lands.
  useEffect(() => {
    if (!log || (shownFor.search === search && shownFor.kind === kind)) return;
    window.clearTimeout(debounce.current);
    debounce.current = window.setTimeout(() => {
      const now = current.current;
      void fetchPage(false, 0, now.search, now.kind, false);
    }, 250);
    return () => window.clearTimeout(debounce.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [search, log === null]);

  const chooseKind = (next: ActivityKind | null) => {
    setKind(next);
    window.clearTimeout(debounce.current);
    if (log) void fetchPage(false, 0, search, next, false);
  };

  const entries = useMemo(() => log?.entries ?? [], [log]);
  const days = useMemo(() => byDay(entries), [entries]);
  // One device in the whole log: its name on every row says nothing.
  const severalDevices = (log?.devices.length ?? 0) > 1;

  const exportAs = async (format: "csv" | "jsonl") => {
    setNotice(null);
    try {
      const path = await saveFileDialog({
        defaultPath: format === "csv" ? "activity-log.csv" : "activity-log.jsonl",
        filters: [
          format === "csv"
            ? { name: "CSV", extensions: ["csv"] }
            : { name: "JSON lines", extensions: ["jsonl"] },
        ],
      });
      if (!path) return;
      const count = await invoke<number>("audit_export", { path, format });
      setNotice(count === 1 ? "Exported 1 event." : `Exported ${count} events.`);
    } catch (e) {
      setNotice(`The log was not exported: ${formatAppError(e)}`);
    }
  };

  const warnings: string[] = [];
  for (const trail of log?.devices ?? []) {
    const missing =
      trail.missing_events.reduce((sum, [from, to]) => sum + (to - from + 1), 0) +
      trail.missing_segments.length;
    if (missing > 0) {
      warnings.push(
        `Records from ${nameOf(trail.device)} are missing. Some may not have reached the copies yet, or were removed.`,
      );
    }
    if (trail.broken_segments.length > 0) {
      warnings.push(
        `The records from ${nameOf(trail.device)} do not follow on from each other in ${trail.broken_segments.length === 1 ? "one place" : `${trail.broken_segments.length} places`}. Some may have been changed.`,
      );
    }
  }
  if (log && log.unreadable > 0) {
    warnings.push(
      log.unreadable === 1
        ? "1 record does not open with this log's key."
        : `${log.unreadable} records do not open with this log's key.`,
    );
  }
  if (log && log.copies_unread.length > 0) {
    warnings.push(
      `Not read from ${log.copies_unread.join(", ")}. Records only there are not shown.`,
    );
  }

  const row = (e: AuditEntry) => {
    const d = describe(e);
    const Icon = ICONS[d.icon];
    const meta = [...(severalDevices ? [nameOf(e.device)] : []), ...d.details];
    return (
      <li key={`${e.device}-${e.i}`} className={`activity-row is-${d.tone}`}>
        <span className="activity-icon" aria-hidden>
          <Icon size={15} />
        </span>
        <span className="activity-body">
          <span className="activity-summary">
            {d.before}
            {d.object && <strong>{d.object}</strong>}
            {d.after}
            {e.n && e.n > 1 ? <span className="activity-times"> ×{e.n}</span> : null}
          </span>
          {meta.length > 0 && <span className="activity-meta">{meta.join(" · ")}</span>}
        </span>
        <time className="activity-time" dateTime={new Date(e.t).toISOString()}>
          {timeOf(e.t)}
        </time>
      </li>
    );
  };

  return (
    <div className="activity-log">
      <div className="activity-toolbar">
        <div className="search-input-wrapper">
          <span className="search-icon">
            <IconSearch size={16} />
          </span>
          <input
            type="text"
            placeholder="Search activity…"
            aria-label="Search activity"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
        <div className="activity-export">
          <button
            type="button"
            className="secondary"
            disabled={!log || log.total === 0}
            onClick={() => void exportAs("csv")}
            title="Export the whole log as CSV"
          >
            <Download size={14} />
            CSV
          </button>
          <button
            type="button"
            className="secondary"
            disabled={!log || log.total === 0}
            onClick={() => void exportAs("jsonl")}
            title="Export the whole log as JSON lines"
          >
            <Download size={14} />
            JSON
          </button>
        </div>
      </div>

      <div className="activity-filters" role="group" aria-label="Show">
        <button
          type="button"
          className={`activity-chip${kind === null ? " is-on" : ""}`}
          aria-pressed={kind === null}
          onClick={() => chooseKind(null)}
        >
          All
        </button>
        {KINDS.map((k) => (
          <button
            key={k}
            type="button"
            className={`activity-chip${kind === k ? " is-on" : ""}`}
            aria-pressed={kind === k}
            onClick={() => chooseKind(k)}
          >
            {KIND_LABELS[k]}
          </button>
        ))}
        {log && (
          <span className="activity-count">
            {search.trim() || kind
              ? `${log.matched} of ${log.total}`
              : `${log.total} ${log.total === 1 ? "event" : "events"}`}
          </span>
        )}
      </div>

      {warnings.length > 0 && (
        <div className="activity-warnings" role="status">
          {warnings.map((w) => (
            <p key={w}>{w}</p>
          ))}
        </div>
      )}
      {error && (
        <p className="hint is-error" role="status">
          {error}
        </p>
      )}
      {notice && <p className="hint">{notice}</p>}
      {loading && !log && <p className="hint">Reading activity…</p>}
      {needsKey && !log && !loading && (
        <div className="activity-empty">
          <p>Reading it asks for one of the organisation&apos;s security keys.</p>
          <button type="button" onClick={() => void load()}>
            Read activity
          </button>
        </div>
      )}
      {!needsKey && !log && !loading && error && (
        <div className="activity-empty">
          <button type="button" onClick={() => void load()}>
            Try again
          </button>
        </div>
      )}
      {log && entries.length === 0 && !loading && (
        <p className="activity-empty">
          {search.trim() || kind ? "Nothing matches." : "Nothing recorded yet."}
        </p>
      )}

      {days.map((day) => (
        <section key={day.label} className="activity-day">
          <h4 className="activity-day-label">{day.label}</h4>
          <ul className="activity-list">{day.events.map(row)}</ul>
        </section>
      ))}

      {log && entries.length < log.matched && (
        <div className="activity-more">
          <button
            type="button"
            className="secondary"
            disabled={loading}
            onClick={() => void fetchPage(false, entries.length, search, kind, true)}
          >
            Show older
          </button>
        </div>
      )}
    </div>
  );
}
