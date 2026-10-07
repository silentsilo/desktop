import { useEffect, useState, type ReactNode } from "react";
import type { MouseEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Calendar, Clock, Copy as CopyIcon, Files, FolderOpen, MoreHorizontal } from "lucide-react";
import type { BackupTargetView } from "../lib/copies";
import {
  copyLines,
  describeSyncState,
  syncStateShort,
  typeLabel,
  type FileCopy,
} from "../lib/fileDetails";
import { fileIconFor, fileKindOf } from "../lib/fileKinds";
import { formatBytes, formatDate } from "../lib/format";
import type { FileSyncState, VaultEntry } from "../lib/types";
import { IconClose, IconExternalLink, IconFolder } from "../ui/Icons";

type Props = {
  entry: VaultEntry;
  /** The folder it sits in; none for the silo's root. */
  location: string | null;
  /** What to call it instead of its own name: the silo's, for its root. */
  title?: string;
  /** How many items a folder holds, when it is the one on screen. */
  count?: number;
  /** The newest change among those items. A folder's own date moves only
   * when it is renamed or moved, never when what is in it changes. */
  lastChange?: number;
  syncState: FileSyncState | null;
  /** Backup storage is set up: the copies are worth naming. */
  syncConfigured: boolean;
  busy: boolean;
  /** Absent for the folder on screen, which is already open. */
  onOpen?: () => void;
  /** The same menu a right-click gives, at the button. */
  onMenu: (e: MouseEvent) => void;
  onClose: () => void;
};

const COPY_STATE: Record<"holds" | "owed" | "unknown", string> = {
  holds: "Has it",
  // Not sent from here or seen there by this computer: another device may
  // have put it there, which the next sync confirms.
  owed: "Not confirmed yet",
  unknown: "",
};

/**
 * What one selected item is, beside the list: what File info used to show
 * in a dialog, and which copies hold the file. Read again whenever the item
 * or its backup state changes; nothing here is kept.
 */
export function FileDetailsPanel({
  entry,
  location,
  title,
  count,
  lastChange,
  syncState,
  syncConfigured,
  busy,
  onOpen,
  onMenu,
  onClose,
}: Props) {
  const isFile = entry.kind === "file";
  const blobId = isFile ? entry.blob_id : null;
  const [targets, setTargets] = useState<BackupTargetView[]>([]);
  const [copies, setCopies] = useState<FileCopy[] | null>(null);

  useEffect(() => {
    if (!syncConfigured) return;
    let live = true;
    void invoke<BackupTargetView[]>("backup_targets_list")
      .then((list) => live && setTargets(list))
      .catch(() => live && setTargets([]));
    return () => {
      live = false;
    };
  }, [syncConfigured]);

  useEffect(() => {
    setCopies(null);
    if (!syncConfigured || !blobId) return;
    let live = true;
    void invoke<FileCopy[]>("file_copies", { blobId })
      .then((list) => live && setCopies(list))
      .catch(() => live && setCopies(null));
    return () => {
      live = false;
    };
    // The state is in the deps so a finished upload shows at once.
  }, [syncConfigured, blobId, syncState]);

  const Icon = isFile ? fileIconFor(entry.name) : null;
  const lines = copies && targets.length > 0 ? copyLines(targets, copies) : [];
  const onlyInBackup = lines.length > 0 && lines.every((line) => line.state === "unknown");

  return (
    <aside className="details-panel" aria-label={isFile ? "File details" : "Folder details"}>
      <button
        type="button"
        className="explorer-icon-btn details-close"
        onClick={onClose}
        title="Hide details"
        aria-label="Hide details"
      >
        <IconClose size={14} />
      </button>

      <div className="details-head">
        <div
          className={`details-icon ${isFile ? `row-file kind-${fileKindOf(entry.name)}` : "row-folder"}`}
          aria-hidden
        >
          {Icon ? <Icon size={36} strokeWidth={1.5} /> : <IconFolder size={38} />}
        </div>
        <h3 className="details-name" title={title ?? entry.name}>
          {title ?? entry.name}
        </h3>
        <p className="details-kind">
          {isFile
            ? `${typeLabel(entry.name)} · ${formatBytes(entry.size_bytes)}`
            : count === undefined
              ? "Folder"
              : `Folder · ${count} ${count === 1 ? "item" : "items"}`}
        </p>
      </div>

      <dl className="details-rows">
        {location !== null && (
          <Row icon={<FolderOpen size={15} />} label="Location">
            {location === "/" ? "Silo root" : location}
          </Row>
        )}
        <Row icon={<Calendar size={15} />} label="Created">
          {formatDate(entry.created_at)}
        </Row>
        {isFile ? (
          <Row icon={<Clock size={15} />} label="Modified">
            {formatDate(entry.updated_at)}
          </Row>
        ) : (
          lastChange !== undefined && (
            <Row icon={<Clock size={15} />} label="Last change inside">
              {formatDate(lastChange)}
            </Row>
          )
        )}
        {isFile && syncState && (
          <Row icon={<CopyIcon size={15} />} label="Backup">
            <span
              className={`details-status is-${syncStateShort(syncState).tone}`}
              title={describeSyncState(syncState)}
            >
              {syncStateShort(syncState).label}
            </span>
          </Row>
        )}
      </dl>

      {lines.length > 0 && (
        <section className="details-copies" aria-label="Copies">
          <h4>Copies</h4>
          {onlyInBackup ? (
            <p className="details-note">
              Put in backup storage by another device. Which copies hold it shows here once this
              computer has the file.
            </p>
          ) : (
            <ul>
              {lines.map((line) => (
                <li key={line.id} className={`details-copy is-${line.state}`}>
                  <span className="details-copy-dot" aria-hidden />
                  <span className="details-copy-name" title={line.name}>
                    {line.name}
                  </span>
                  <span className="details-copy-state">{COPY_STATE[line.state]}</span>
                </li>
              ))}
            </ul>
          )}
        </section>
      )}

      <div className="details-actions">
        {onOpen ? (
          <button type="button" className="details-open" disabled={busy} onClick={onOpen}>
            {isFile ? <IconExternalLink size={15} /> : <IconFolder size={15} />}
            Open
          </button>
        ) : (
          <span className="details-open-spacer" />
        )}
        <button
          type="button"
          className="secondary details-more"
          onClick={onMenu}
          title="More actions"
          aria-label="More actions"
        >
          <MoreHorizontal size={17} />
        </button>
      </div>
    </aside>
  );
}

function Row({ icon, label, children }: { icon: ReactNode; label: string; children: ReactNode }) {
  return (
    <div className="details-row">
      <span className="details-row-icon" aria-hidden>
        {icon}
      </span>
      <div>
        <dt>{label}</dt>
        <dd>{children}</dd>
      </div>
    </div>
  );
}

/** Several items selected: how many, of what, how large, and their menu. */
export function SelectionDetails({
  entries,
  onMenu,
  onClose,
}: {
  entries: VaultEntry[];
  onMenu: (e: MouseEvent) => void;
  onClose: () => void;
}) {
  const files = entries.filter((e) => e.kind === "file");
  const folders = entries.length - files.length;
  const bytes = files.reduce((sum, f) => sum + (f.kind === "file" ? f.size_bytes : 0), 0);
  const parts = [
    files.length > 0 && `${files.length} ${files.length === 1 ? "file" : "files"}`,
    folders > 0 && `${folders} ${folders === 1 ? "folder" : "folders"}`,
  ].filter(Boolean);
  return (
    <aside className="details-panel" aria-label="Selection">
      <button
        type="button"
        className="explorer-icon-btn details-close"
        onClick={onClose}
        title="Hide details"
        aria-label="Hide details"
      >
        <IconClose size={14} />
      </button>
      <div className="details-head">
        <div className="details-icon" aria-hidden>
          <Files size={36} strokeWidth={1.5} />
        </div>
        <h3 className="details-name">{entries.length} items selected</h3>
        <p className="details-kind">
          {parts.join(" and ")}
          {files.length > 0 && ` · ${formatBytes(bytes)}`}
        </p>
      </div>
      <div className="details-actions">
        <span className="details-open-spacer" />
        <button
          type="button"
          className="secondary details-more"
          onClick={onMenu}
          title="Actions for these items"
          aria-label="Actions for these items"
        >
          <MoreHorizontal size={17} />
        </button>
      </div>
    </aside>
  );
}
