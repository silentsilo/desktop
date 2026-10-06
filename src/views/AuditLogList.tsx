import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Download, History } from "lucide-react";
import { save as saveFileDialog } from "../lib/dialog";
import { formatDate } from "../lib/format";
import { formatAppError } from "../lib/errors";
import { IconSearch } from "../ui/Icons";
import type { AuditEntry, AuditLog } from "../lib/types";

type Props = {
  devices: { id: string; label: string | null; system_name: string | null }[];
};

/** Rows shown before "Show more". */
const PAGE = 100;

/** What an event carries besides its name, in the order worth reading. */
const DETAIL_KEYS = [
  "field",
  "site",
  "entry",
  "key",
  "by",
  "now",
  "count",
  "files",
  "kept",
  "format",
  "what",
];

function details(entry: AuditEntry): string[] {
  const x = entry.x ?? {};
  const known = DETAIL_KEYS.filter((k) => x[k] !== undefined && x[k] !== "").map((k) =>
    k === "count" || k === "files" || k === "kept" ? `${k} ${String(x[k])}` : String(x[k]),
  );
  const other = Object.keys(x)
    .filter((k) => !DETAIL_KEYS.includes(k))
    .map((k) => `${k}: ${JSON.stringify(x[k])}`);
  return [...known, ...other];
}

/**
 * The silo's activity log, read from this computer and every copy, newest
 * first. Says what is missing rather than leaving it out silently: a hole
 * in a device's run is what a reader of a log most needs to see.
 */
export function AuditLogList({ devices }: Props) {
  const [log, setLog] = useState<AuditLog | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [shown, setShown] = useState(PAGE);
  const [notice, setNotice] = useState<string | null>(null);

  const nameOf = useCallback(
    (id: string) => {
      const device = devices.find((d) => d.id === id);
      return device?.label || device?.system_name || `Device ${id.slice(0, 8)}`;
    },
    [devices],
  );

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setLog(await invoke<AuditLog>("audit_read"));
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const filtered = useMemo(() => {
    const entries = log?.entries ?? [];
    const term = search.trim().toLowerCase();
    if (!term) return entries;
    return entries.filter((e) =>
      [e.what, e.l ?? "", nameOf(e.device), ...details(e)].some((v) =>
        v.toLowerCase().includes(term),
      ),
    );
  }, [log, nameOf, search]);

  const exportAs = async (format: "csv" | "jsonl") => {
    setNotice(null);
    const path = await saveFileDialog({
      defaultPath: format === "csv" ? "activity-log.csv" : "activity-log.jsonl",
      filters: [
        format === "csv"
          ? { name: "CSV", extensions: ["csv"] }
          : { name: "JSON lines", extensions: ["jsonl"] },
      ],
    });
    if (!path) return;
    try {
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

  return (
    <div className="panel-section">
      <h3>
        <History size={16} />
        Activity
      </h3>
      <p>
        What the activity log holds, from all the devices that keep it. Times come from each
        device&apos;s own clock, so they can be slightly off.
      </p>

      <div className="search-input-wrapper">
        <span className="search-icon">
          <IconSearch size={16} />
        </span>
        <input
          type="text"
          placeholder="Search the log…"
          aria-label="Search the activity log"
          value={search}
          onChange={(e) => {
            setSearch(e.target.value);
            setShown(PAGE);
          }}
        />
      </div>

      {warnings.map((w) => (
        <p key={w} className="hint is-warning">
          {w}
        </p>
      ))}
      {error && (
        <p className="hint is-error" role="status">
          {error}
        </p>
      )}
      {loading && !log && <p className="hint">Reading the log…</p>}
      {log && filtered.length === 0 && (
        <p className="hint">
          {search.trim() ? "Nothing in the log matches." : "Nothing recorded yet."}
        </p>
      )}

      {filtered.length > 0 && (
        <ul className="activity-list">
          {filtered.slice(0, shown).map((e) => (
            <li key={`${e.device}-${e.i}`} className="activity-row">
              <span className="activity-summary">{e.l ? `${e.what}: ${e.l}` : e.what}</span>
              <span className="hint activity-meta">
                {[nameOf(e.device), formatDate(e.t), ...details(e)].join(" · ")}
                {e.n && e.n > 1 ? ` · ${e.n} times` : ""}
              </span>
            </li>
          ))}
        </ul>
      )}

      <div className="actions">
        {filtered.length > shown && (
          <button type="button" className="secondary" onClick={() => setShown((n) => n + PAGE)}>
            Show older
          </button>
        )}
        <button
          type="button"
          className="secondary"
          disabled={!log || log.entries.length === 0}
          onClick={() => void exportAs("csv")}
        >
          <Download size={14} />
          Export CSV
        </button>
        <button
          type="button"
          className="secondary"
          disabled={!log || log.entries.length === 0}
          onClick={() => void exportAs("jsonl")}
        >
          <Download size={14} />
          Export JSON lines
        </button>
      </div>
      {notice && <p className="hint">{notice}</p>}
    </div>
  );
}
