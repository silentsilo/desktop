import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Copy, HardDrive, Laptop, Plus, Trash2, Truck, X } from "lucide-react";
import { ConfirmDialog } from "./ConfirmDialog";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { formatAppError } from "../lib/errors";
import {
  copyState,
  currentCopies,
  protectionWarning,
  type BackupTargetView,
  type Protection,
} from "../lib/copies";
import {
  EMPTY_STORE_DRAFT,
  missingStoreFields,
  StoreConfigForm,
  storeDraftPayload,
  type StoreDraft,
} from "./StoreConfigForm";
import type { StoreConfigView } from "../lib/types";

/** A short phrase naming where a target points, for the row's title. */
function whereIs(config: StoreConfigView): string {
  switch (config.kind) {
    case "s3":
      return config.prefix ? `${config.bucket}/${config.prefix}` : config.bucket;
    case "folder":
      return config.path;
    case "web-dav":
      return config.url;
    default:
      return `${config.username}@${config.host}`;
  }
}

type Props = {
  busy: boolean;
  /** Whether this computer holds every file, not just the index. */
  fullCopy: boolean;
  /** Lets the shell refresh its status line after a target changes. */
  onActivity: () => void;
};

/**
 * Every copy of this silo, and how far behind each one is.
 *
 * 3-2-1 is a practice rather than a setting, and the way it fails is that
 * someone believes they have three copies when a disk has been in a drawer
 * since spring. This panel exists to make that visible without being asked:
 * each copy shows its age, so an unplugged one reads as "last written 47
 * days ago" rather than as an error nobody can distinguish from a bad
 * afternoon on the network.
 */
export function CopiesPanel({ busy, fullCopy, onActivity }: Props) {
  const [targets, setTargets] = useState<BackupTargetView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState<StoreDraft>(EMPTY_STORE_DRAFT);
  const [label, setLabel] = useState("");
  const [archive, setArchive] = useState(false);
  /// What the place says about resisting deletion, once it has been asked.
  /// Null means not asked yet, which is different from "answered no".
  const [protection, setProtection] = useState<Protection | null>(null);
  const [working, setWorking] = useState(false);
  /// The target being filled right now, and how far through. Seeding runs
  /// for hours on the volumes it exists for, and a spinner with no number on
  /// it is what makes people pull the cable.
  const [seeding, setSeeding] = useState<string | null>(null);
  const [seedProgress, setSeedProgress] = useState<[number, number] | null>(null);
  const [seedCancelling, setSeedCancelling] = useState(false);
  /// The target Remove is asking about. Removing a copy is not destructive
  /// to data, but it silently stops a backup, which deserves one question.
  const [confirmRemove, setConfirmRemove] = useState<BackupTargetView | null>(null);
  const [note, setNote] = useState<string | null>(null);
  // One instant for the whole list, so two rows written a second apart do not
  // disagree about what "now" was.
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));

  const refresh = useCallback(async () => {
    try {
      setTargets(await invoke<BackupTargetView[]>("backup_targets_list"));
      setNow(Math.floor(Date.now() / 1000));
    } catch (e) {
      setError(formatAppError(e));
      setTargets([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEventSubscription(
    () =>
      listen<[number, number]>("seed-progress", (event) => setSeedProgress(event.payload)),
    [],
  );

  // Every pass, not only the ones started from this screen. What each copy
  // is doing changes when the background sync finishes, and reading it once
  // on mount is why a copy that had just caught up still read as behind
  // until the page was left and reopened.
  useEventSubscription(() => listen("sync-report", () => void refresh()), [refresh]);

  const add = async () => {
    const missing = missingStoreFields(draft, false);
    if (missing.length > 0) {
      setError(`Still needed: ${missing.join(", ")}.`);
      return;
    }
    setError(null);
    setWorking(true);
    try {
      await invoke("backup_target_add", {
        config: storeDraftPayload(draft),
        label: label.trim(),
        archive,
      });
      setAdding(false);
      setDraft(EMPTY_STORE_DRAFT);
      setLabel("");
      setArchive(false);
      setProtection(null);
      await refresh();
      onActivity();
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setWorking(false);
    }
  };

  /// Asks the place itself what it does about deletion.
  ///
  /// Only once append-only has been ticked and the form is complete enough
  /// to point at something, because it is two network calls and it changes
  /// nothing for a working target. Tied to the draft rather than to the
  /// tick: someone ticks the box and then types the bucket name, so probing
  /// only on the tick would ask about nothing and never ask again.
  ///
  /// A failure answers "no protection", which is the same answer a provider
  /// that does not implement the calls gives. Claiming protection nobody
  /// confirmed is the failure this exists to prevent.
  useEffect(() => {
    if (!archive || missingStoreFields(draft, false).length > 0) {
      setProtection(null);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      void invoke<Protection>("backup_target_protection", { config: storeDraftPayload(draft) })
        .then((found) => {
          if (!cancelled) setProtection(found);
        })
        .catch(() => {
          if (!cancelled) setProtection({ versioning: false, object_lock: false });
        });
      // Late enough that typing a bucket name does not ask once per letter.
    }, 600);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [archive, draft]);

  /// Fills one place from another over a cable.
  ///
  /// Offered on the non-primary rows because that is where it is needed: the
  /// second copy of a large silo is the one that would otherwise take weeks
  /// of home upload. The source is the primary target, which is the one that
  /// already has everything.
  const seed = async (id: string) => {
    setError(null);
    setNote(null);
    setSeeding(id);
    setSeedCancelling(false);
    try {
      const copied = await invoke<number>("backup_target_seed", { from: list[0]?.id ?? "", to: id });
      setNote(
        copied === 0
          ? "Already had everything."
          : `Copied ${copied} object${copied === 1 ? "" : "s"} across.`,
      );
      await refresh();
    } catch (e) {
      // Compared raw rather than after formatAppError, which rewrites
      // anything containing "cancelled" into a FIDO-prompt message.
      if (String(e) === "cancelled") {
        setNote("Stopped. What already copied stays there, and running it again carries on.");
        await refresh();
      } else {
        setError(formatAppError(e));
      }
    } finally {
      setSeeding(null);
      setSeedProgress(null);
      setSeedCancelling(false);
    }
  };

  const cancelSeed = () => {
    setSeedCancelling(true);
    void invoke("cancel_seed").catch(() => {});
  };

  const remove = async (id: string) => {
    setError(null);
    setWorking(true);
    try {
      await invoke("backup_target_remove", { id });
      await refresh();
      onActivity();
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setWorking(false);
    }
  };

  const list = targets ?? [];
  // This computer counts only when it holds the content. Without that
  // setting a silo is an index that fetches files when you open them, and
  // calling it a copy would be the exact overstatement this panel is for.
  const current = currentCopies(list, now) + (fullCopy ? 1 : 0);
  const places = list.length + (fullCopy ? 1 : 0);
  const media = new Set(list.map((t) => t.config.kind)).size + (fullCopy ? 1 : 0);

  return (
    <div className="panel-section">
      <h3>
        <Copy size={16} />
        Copies
      </h3>
      <p>
        The rule worth keeping is three copies, on two kinds of storage, one of them somewhere
        else. Right now this silo has {current} of {places}{" "}
        {places === 1 ? "place" : "places"} up to date, across {media}{" "}
        {media === 1 ? "kind" : "kinds"} of storage.
        {list.length > 0 ? " At least one of them is off this computer." : ""}
      </p>

      <ul className="key-list copies-list">
        <li className="key-list-item">
          <span className="copy-icon" aria-hidden>
            <Laptop size={16} />
          </span>
          <div className="protected-row-text">
            <strong>This computer</strong>
            <span className="hint">
              {fullCopy
                ? "Holds every file, so it counts as a copy."
                : "Holds the file list and fetches contents when you open them, so it is an index rather than a copy."}
            </span>
          </div>
        </li>

        {list.map((target) => {
          const state = copyState(target, now);
          return (
            <li key={target.id} className="key-list-item">
              <span className={`copy-icon is-${state.health}`} aria-hidden>
                <HardDrive size={16} />
              </span>
              <div className="protected-row-text">
                <strong>
                  {target.label || whereIs(target.config)}
                  {target.archive && <span className="copy-tag">append-only</span>}
                </strong>
                <span className={`hint copy-state is-${state.health}`}>{state.headline}</span>
                <span className="hint">{state.detail}</span>
                {target.archive && (
                  <span className="hint">
                    Nothing is ever deleted here, so emptying the trash and tidying old history
                    leave it untouched and it keeps growing.
                  </span>
                )}
              </div>
              {/* The first target is the connection the Backup screen edits.
                  Removing it there is called Disconnect and clears
                  everything, so offering a second way to do it from here
                  would mean two buttons with different consequences. */}
              {!target.primary && (
                <div className="key-list-actions">
                  <button
                    type="button"
                    className="secondary"
                    disabled={busy || working || seeding !== null}
                    onClick={() => void seed(target.id)}
                    title="Copy everything from the first place into this one"
                  >
                    {seeding === target.id ? (
                      <span className="spinner" aria-hidden />
                    ) : (
                      <Truck size={14} />
                    )}
                    {seeding === target.id
                      ? seedProgress
                        ? `${seedProgress[0]} of ${seedProgress[1]}…`
                        : "Copying…"
                      : "Fill from the first copy"}
                  </button>
                  {seeding === target.id && (
                    <button
                      type="button"
                      className="secondary"
                      disabled={seedCancelling}
                      onClick={cancelSeed}
                      title="Stop copying. What already arrived stays, and running it again carries on."
                    >
                      <X size={14} />
                      {seedCancelling ? "Stopping…" : "Stop"}
                    </button>
                  )}
                  <button
                    type="button"
                    className="secondary"
                    disabled={busy || working || seeding !== null}
                    onClick={() => setConfirmRemove(target)}
                    title="Stop backing up to this place"
                  >
                    <Trash2 size={14} />
                    Remove
                  </button>
                </div>
              )}
            </li>
          );
        })}
      </ul>

      {seeding !== null && seedProgress && seedProgress[1] > 0 && (
        <div className="progress-row" role="status">
          <p className="hint">
            Copying: {seedProgress[0]} of {seedProgress[1]} objects.
          </p>
          <div className="progress-track">
            <div
              className="progress-fill"
              style={{ width: `${Math.min(100, (seedProgress[0] / seedProgress[1]) * 100)}%` }}
            />
          </div>
        </div>
      )}

      {error && (
        <p className="hint is-error" role="status">
          {error}
        </p>
      )}

      {note && !error && (
        <p className="hint success-msg" role="status">
          {note}
        </p>
      )}

      {list.length > 1 && (
        <p className="hint">
          Filling one place from another copies the files directly, so a large silo can go onto
          an external disk over a cable instead of over your connection. It moves encrypted files
          and never needs your key, it can be stopped at any point, and running it again carries
          on from where it stopped.
        </p>
      )}

      {adding ? (
        <div className="copies-add">
          <label className="field">
            <span>What to call it</span>
            <input
              value={label}
              disabled={working}
              placeholder="Disc extern, birou"
              onChange={(e) => setLabel(e.target.value)}
            />
          </label>
          <StoreConfigForm
            draft={draft}
            onChange={setDraft}
            hasStoredSecret={false}
            busy={working}
          />
          <label className="confirm-option">
            <input
              type="checkbox"
              checked={archive}
              disabled={working}
              onChange={(e) => setArchive(e.target.checked)}
            />
            <span>
              Never delete anything here
              <span className="hint">
                For a copy meant to survive this computer being taken over. SilentSilo only ever
                adds to it: emptying the trash, tidying old history and clearing unused content
                all skip it, so it grows for ever and you pay for that. Pair it with a bucket
                that has object lock, or with credentials that have no permission to delete.
              </span>
            </span>
          </label>

          {protectionWarning(protection, archive) && (
            <p className="hint is-error" role="status">
              {protectionWarning(protection, archive)}
            </p>
          )}

          <p className="hint">
            It is written to before it is saved. A place that cannot be written to is not a copy,
            and finding that out on the next pass means believing you have one for however long
            that takes.
          </p>
          <div className="actions">
            <button type="button" disabled={working} onClick={() => void add()}>
              {working ? <span className="spinner" aria-hidden /> : <Plus size={15} />}
              {working ? "Checking…" : "Add this place"}
            </button>
            <button
              type="button"
              className="secondary"
              disabled={working}
              onClick={() => {
                setError(null);
                setAdding(false);
              }}
            >
              <X size={15} />
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <div className="actions">
          <button type="button" disabled={busy || working} onClick={() => setAdding(true)}>
            <Plus size={15} />
            Add another place
          </button>
        </div>
      )}

      {confirmRemove && (
        <ConfirmDialog
          title="Stop backing up to this place?"
          message={`“${confirmRemove.label || whereIs(confirmRemove.config)}” stops receiving copies of this silo. What is already there is left alone, and adding the place back later picks up from where it stopped.`}
          confirmLabel="Remove"
          danger
          busy={working}
          onConfirm={() => {
            const id = confirmRemove.id;
            setConfirmRemove(null);
            void remove(id);
          }}
          onCancel={() => setConfirmRemove(null)}
        />
      )}
    </div>
  );
}
