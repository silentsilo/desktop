import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { History } from "lucide-react";
import { formatDate } from "../lib/format";
import { formatAppError } from "../lib/errors";
import { IconSearch } from "../ui/Icons";
import type { ActivityCursor, ActivityPage } from "../lib/types";

type Props = {
  devices: { id: string; label: string | null; system_name: string | null }[];
};

/** One screenful. Enough to fill the panel without asking for a log's worth. */
const PAGE_SIZE = 50;

/**
 * What has happened to this silo, newest first.
 *
 * Called Activity rather than Audit log, and the difference is not wording.
 * An audit log is evidence: complete, ordered by a clock you can trust, hard
 * to forge. None of that holds here, because every record comes from a device
 * holding the vault key and any of them can write whatever it likes. What
 * this answers is the ordinary question, what happened to my files and which
 * machine did it, and that is the whole claim.
 */
export function ActivityList({ devices }: Props) {
  const [search, setSearch] = useState("");
  const [page, setPage] = useState<ActivityPage | null>(null);
  const [entries, setEntries] = useState<ActivityPage["entries"]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /// Guards against an older answer landing after a newer one. Typing is
  /// faster than the log is, so two requests are often in flight and the
  /// slower one must not overwrite the list.
  const generation = useRef(0);

  const fetchPage = useCallback(
    async (term: string, after: ActivityCursor | null, append: boolean) => {
      const mine = ++generation.current;
      setLoading(true);
      setError(null);
      try {
        const result = await invoke<ActivityPage>("vault_activity", {
          query: { search: term, after, limit: PAGE_SIZE },
        });
        if (generation.current !== mine) return;
        setPage(result);
        setEntries((previous) => (append ? [...previous, ...result.entries] : result.entries));
      } catch (e) {
        if (generation.current !== mine) return;
        setError(formatAppError(e));
        setPage(null);
        setEntries([]);
      } finally {
        if (generation.current === mine) setLoading(false);
      }
    },
    [],
  );

  // Debounced, because a search decodes records rather than reading a column,
  // and firing one per keystroke would have the log scanned five times for a
  // word typed once.
  useEffect(() => {
    const timer = setTimeout(() => void fetchPage(search, null, false), 200);
    return () => clearTimeout(timer);
  }, [search, fetchPage]);

  const nameFor = (id: string, label: string) => {
    if (label) return label;
    const device = devices.find((d) => d.id === id);
    return device?.label || device?.system_name || `device ${id.slice(0, 6)}`;
  };

  return (
    <div className="panel-section">
      <h3>
        <History size={16} />
        Activity
      </h3>
      <p>
        Every change recorded in this silo, in the order all your devices agree on. The time beside
        each one comes from the clock of the machine that made it, so treat it as a label rather
        than a measurement. Unlocking is not listed: it happens on one device and is never shared.
      </p>

      <div className="search-input-wrapper">
        <span className="search-icon">
          <IconSearch size={16} />
        </span>
        <input
          type="text"
          placeholder="Search this history…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
      </div>

      {error && (
        <p className="hint is-error" role="status">
          {error}
        </p>
      )}

      {page === null && loading && <p className="hint">Reading the log…</p>}

      {page !== null && entries.length === 0 && (
        <p className="hint">
          {search.trim() ? `Nothing here matches “${search.trim()}”.` : "Nothing has changed in this silo yet."}
        </p>
      )}

      {entries.length > 0 && (
        <ul className="activity-list">
          {entries.map((entry) => (
            <li
              key={entry.op_id}
              className={entry.unknown ? "activity-row unknown" : "activity-row"}
            >
              <span className="activity-summary">{entry.summary}</span>
              <span className="hint activity-meta">
                {nameFor(entry.device_id, entry.device_label)} · {formatDate(entry.at)}
              </span>
            </li>
          ))}
        </ul>
      )}

      {page?.next && (
        <div className="actions">
          <button
            type="button"
            className="secondary"
            disabled={loading}
            onClick={() => void fetchPage(search, page.next, true)}
          >
            {loading ? "Reading…" : "Show older"}
          </button>
        </div>
      )}

      {/* Said plainly rather than left to be inferred from the oldest row on
          screen, which otherwise reads as the day the silo began. */}
      {page !== null && page.truncated_before > 0 && !page.next && (
        <p className="hint">
          Older changes are no longer stored. The history kept here starts after the silo was
          compacted to save space; your files are unaffected.
        </p>
      )}

      {page?.total !== null && page?.total !== undefined && (
        <p className="hint">
          {page.total} change{page.total === 1 ? "" : "s"} recorded in total.
        </p>
      )}
    </div>
  );
}
