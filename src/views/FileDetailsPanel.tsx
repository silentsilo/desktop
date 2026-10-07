import { useEffect, useState } from "react";
import type { MouseEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Calendar, Clock, Copy as CopyIcon, FolderOpen, MoreHorizontal } from "lucide-react";
import type { BackupTargetView } from "../lib/copies";
import { copyLines, describeSyncState, typeLabel, type FileCopy } from "../lib/fileDetails";
import { fileIconFor, fileKindOf } from "../lib/fileKinds";
import { formatBytes, formatDate } from "../lib/format";
import type { FileSyncState, VaultEntry } from "../lib/types";
import { IconClose, IconExternalLink, IconFolder } from "../ui/Icons";

type Props = {
  entry: VaultEntry;
  /** The folder it sits in. */
  location: string;
  syncState: FileSyncState | null;
  /** Backup storage is set up: the copies are worth naming. */
  syncConfigured: boolean;
  busy: boolean;
  onOpen: () => void;
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
          {Icon ? <Icon size={40} strokeWidth={1.4} /> : <IconFolder size={42} />}
        </div>
        <h3 className="details-name" title={entry.name}>
          {entry.name}
        </h3>
        <p className="details-kind">
          {isFile ? `${typeLabel(entry.name)} · ${formatBytes(entry.size_bytes)}` : "Folder"}
        </p>
      </div>

      <dl className="details-rows">
        <div className="details-row">
          <FolderOpen size={15} aria-hidden />
          <dt>Location</dt>
          <dd>{location === "/" ? "Silo root" : location}</dd>
        </div>
        <div className="details-row">
          <Calendar size={15} aria-hidden />
          <dt>Created</dt>
          <dd>{formatDate(entry.created_at)}</dd>
        </div>
        <div className="details-row">
          <Clock size={15} aria-hidden />
          <dt>Modified</dt>
          <dd>{formatDate(entry.updated_at)}</dd>
        </div>
        {isFile && syncState && (
          <div className="details-row">
            <CopyIcon size={15} aria-hidden />
            <dt>Backup</dt>
            <dd>{describeSyncState(syncState)}</dd>
          </div>
        )}
      </dl>

      {lines.length > 0 && (
        <section className="details-copies" aria-label="Copies">
          <h4>Copies</h4>
          {onlyInBackup ? (
            <p className="hint">
              Another device put this file in backup storage. Which copies hold it shows here once
              this computer has the file.
            </p>
          ) : (
            <ul>
              {lines.map((line) => (
                <li key={line.id} className={`details-copy is-${line.state}`}>
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
        <button type="button" disabled={busy} onClick={onOpen}>
          {isFile ? <IconExternalLink size={14} /> : <IconFolder size={14} />}
          Open
        </button>
        <button
          type="button"
          className="secondary details-more"
          onClick={onMenu}
          title="More actions"
          aria-label="More actions"
        >
          <MoreHorizontal size={16} />
        </button>
      </div>
    </aside>
  );
}
