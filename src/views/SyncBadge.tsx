import { CloudAlert, CloudCheck, CloudDownload, CloudUpload } from "lucide-react";
import type { FileSyncState } from "../lib/types";
import { t, useLocale, type Key } from "../i18n";

/**
 * Where one file's content is, in a mark small enough to sit in a row.
 *
 * Three states, because three is what the backend can actually distinguish:
 * a blob that has not reached the backup yet, one that has and is still
 * cached here, and one that has and was evicted to make room. The fourth
 * conceivable state, "not backed up and not here", cannot exist: a blob is
 * never evicted before it is confirmed uploaded.
 */
/// All three read as clouds, so the column scans as one idea (where is this
/// file?) and the arrow direction carries the difference: going up means it
/// still owes the backup, coming down means the backup owes this machine.
const LOOK: Record<
  Exclude<FileSyncState, "local-only">,
  { Icon: typeof CloudCheck; label: Key; title: Key }
> = {
  pending: {
    Icon: CloudUpload,
    label: "start.badge_pending",
    title: "start.badge_pending_title",
  },
  "backed-up": {
    Icon: CloudCheck,
    label: "start.badge_backed_up",
    title: "start.badge_backed_up_title",
  },
  "remote-only": {
    Icon: CloudDownload,
    label: "start.badge_remote_only",
    title: "start.badge_remote_only_title",
  },
  uploading: {
    Icon: CloudUpload,
    label: "start.badge_uploading",
    title: "start.badge_uploading_title",
  },
  downloading: {
    Icon: CloudDownload,
    label: "start.badge_downloading",
    title: "start.badge_downloading_title",
  },
  absent: {
    Icon: CloudAlert,
    label: "start.badge_missing",
    title: "start.badge_missing_title",
  },
};

export function SyncBadge({ state, compact }: { state: FileSyncState; compact?: boolean }) {
  useLocale();
  if (state === "local-only") return null;
  const { Icon } = LOOK[state];
  const label = t(LOOK[state].label);
  const title = t(LOOK[state].title);
  return (
    <span className={`sync-badge sync-${state}`} title={title} aria-label={title}>
      <Icon size={12} aria-hidden />
      {!compact && <span aria-hidden>{label}</span>}
    </span>
  );
}
