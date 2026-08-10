import type { FileSyncState, VaultEntry } from "../lib/types";
import { formatBytes, formatDate } from "../lib/format";
import { useModal } from "../hooks/useModal";

type Props = {
  entry: VaultEntry;
  /** The folder the entry sits in, so Info answers "where is this" too. */
  location?: string;
  /** Where the content is right now, when a backup exists to compare with. */
  syncState?: FileSyncState | null;
  onClose: () => void;
};

/// The same three states the row badges show, in words rather than a glyph:
/// this dialog is where someone comes for the sentence.
function describeSyncState(state: FileSyncState): string {
  if (state === "backed-up") return "On this computer and in the backup.";
  if (state === "pending") return "On this computer, waiting to be backed up.";
  return "In the backup only. It downloads when opened.";
}

export function FileInfoDialog({ entry, location, syncState, onClose }: Props) {
  const isFile = entry.kind === "file";
  const cardRef = useModal(onClose);
  const title = isFile ? "File info" : "Folder info";

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={cardRef}
        className="modal-card"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
      >
        <h3 className="modal-title">{title}</h3>
        <div className="modal-body info-body">
          <div className="info-row">
            <span className="info-label">Name</span>
            <span>{entry.name || "/"}</span>
          </div>
          {location !== undefined && (
            <div className="info-row">
              <span className="info-label">Where</span>
              <span>{location === "/" ? "Silo root" : location}</span>
            </div>
          )}
          {isFile && (
            <div className="info-row">
              <span className="info-label">Size</span>
              <span>{formatBytes(entry.size_bytes)}</span>
            </div>
          )}
          {isFile && entry.mime_type && (
            <div className="info-row">
              <span className="info-label">Type</span>
              <span>{entry.mime_type}</span>
            </div>
          )}
          {isFile && syncState && (
            <div className="info-row">
              <span className="info-label">Backup</span>
              <span>{describeSyncState(syncState)}</span>
            </div>
          )}
          <div className="info-row">
            <span className="info-label">Created</span>
            <span>{formatDate(entry.created_at)}</span>
          </div>
          <div className="info-row">
            <span className="info-label">Modified</span>
            <span>{formatDate(entry.updated_at)}</span>
          </div>
        </div>
        <div className="modal-actions">
          <button type="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
