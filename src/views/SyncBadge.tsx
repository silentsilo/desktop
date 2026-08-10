import { CloudCheck, CloudDownload, CloudUpload } from "lucide-react";
import type { FileSyncState } from "../lib/types";

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
  { Icon: typeof CloudCheck; label: string; title: string }
> = {
  pending: {
    Icon: CloudUpload,
    label: "Waiting",
    title: "Not backed up yet. This computer holds the only copy.",
  },
  "backed-up": {
    Icon: CloudCheck,
    label: "Backed up",
    title: "Backed up, and kept on this computer for offline use.",
  },
  "remote-only": {
    Icon: CloudDownload,
    label: "In backup only",
    title: "Backed up. The content downloads when you open it.",
  },
};

export function SyncBadge({ state, compact }: { state: FileSyncState; compact?: boolean }) {
  if (state === "local-only") return null;
  const { Icon, label, title } = LOOK[state];
  return (
    <span className={`sync-badge sync-${state}`} title={title} aria-label={title}>
      <Icon size={12} aria-hidden />
      {!compact && <span aria-hidden>{label}</span>}
    </span>
  );
}
