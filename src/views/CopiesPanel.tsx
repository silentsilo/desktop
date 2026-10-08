import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Copy, HardDrive, Laptop, LogIn, Plus, Trash2, Truck, X } from "lucide-react";
import { ConfirmDialog } from "./ConfirmDialog";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { useLasting } from "../lib/lasting";
import { coalesceLatest, coalesceRuns } from "../lib/coalesce";
import { formatAppError } from "../lib/errors";
import {
  copyState,
  currentCopies,
  protectionWarning,
  seedHeadline,
  seedLabel,
  seedPercent,
  whereIs,
  type BackupTargetView,
  type Protection,
} from "../lib/copies";
import {
  discardSignIns,
  EMPTY_STORE_DRAFT,
  missingStoreFields,
  StoreConfigForm,
  storeDraftPayload,
  type StoreDraft,
} from "./StoreConfigForm";
import {
  isCloudKind,
  isCloudView,
  type CloudKind,
  type CloudSignIn,
  type SeedProgress,
} from "../lib/types";
import { CLOUD_NAME } from "../lib/cloud";
import { t, useLocale } from "../i18n";

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
const never = () => false;

export function CopiesPanel({ busy, fullCopy, onActivity }: Props) {
  useLocale();
  const [targets, setTargets] = useState<BackupTargetView[] | null>(null);
  // Lasting, like the work behind them: see lib/lasting.
  const [error, setError] = useLasting<string | null>("copies.error", null, never);
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState<StoreDraft>(EMPTY_STORE_DRAFT);
  const [label, setLabel] = useState("");
  const [archive, setArchive] = useState(false);
  /// What the place says about resisting deletion, once it has been asked.
  /// Null means not asked yet, which is different from "answered no".
  const [protection, setProtection] = useState<Protection | null>(null);
  const [working, setWorking] = useLasting<boolean>("copies.working", false, Boolean);
  /// The target being filled right now, and how far through. Seeding runs
  /// for hours on the volumes it exists for, and a spinner with no number on
  /// it is what makes people pull the cable.
  const [seeding, setSeeding] = useLasting<string | null>("copies.seeding", null, Boolean);
  const [seedProgress, setSeedProgress] = useLasting<SeedProgress | null>(
    "copies.seed_progress",
    null,
    Boolean,
  );
  const [seedCancelling, setSeedCancelling] = useLasting<boolean>("copies.seed_cancelling", false, Boolean);
  /// The target Remove is asking about. Removing a copy is not destructive
  /// to data, but it silently stops a backup, which deserves one question.
  const [confirmRemove, setConfirmRemove] = useState<BackupTargetView | null>(null);
  const [note, setNote] = useLasting<string | null>("copies.note", null, never);
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
  }, [setError]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // One refresh at a time, with one more run for whatever arrived while it
  // was in flight. A seed ends with a sync pass, and a pass per open silo
  // reports in a burst; a call per report is a queue of round trips that all
  // read the same state and only the last of which anyone reads.
  const refreshRef = useRef(refresh);
  refreshRef.current = refresh;
  const queueRefresh = useMemo(() => coalesceRuns(() => refreshRef.current()), []);

  // Filling a copy reports as it goes, four times a second while one large
  // object moves and once per object otherwise, and the volumes this exists
  // for run to hundreds of thousands of objects. Every value but the newest
  // is already stale by the time a frame could draw it, so only the newest
  // is kept: without this the panel re-rendered once per report, and the
  // Stop button was competing with its own progress line for frames.
  const seedTicker = useMemo(() => coalesceLatest<SeedProgress>(setSeedProgress), [setSeedProgress]);
  useEffect(() => seedTicker.stop, [seedTicker]);
  useEventSubscription(
    () => listen<SeedProgress>("seed-progress", (event) => seedTicker.push(event.payload)),
    [seedTicker],
  );

  // Every pass, not only the ones started from this screen. What each copy
  // is doing changes when the background sync finishes, and reading it once
  // on mount is why a copy that had just caught up still read as behind
  // until the page was left and reopened.
  useEventSubscription(() => listen("sync-report", () => queueRefresh()), [queueRefresh]);

  const add = async () => {
    const missing = missingStoreFields(draft, false);
    if (missing.length > 0) {
      setError(t("backup.still_needed", { fields: missing.join(", ") }));
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
        copied === 0 ? t("backup.copies_already") : t("backup.copies_copied", { count: copied }),
      );
      await refresh();
    } catch (e) {
      // Compared raw rather than after formatAppError, which rewrites
      // anything containing "cancelled" into a FIDO-prompt message.
      if (String(e) === "cancelled") {
        setNote(t("backup.copies_stopped"));
        await refresh();
      } else {
        setError(formatAppError(e));
      }
    } finally {
      setSeeding(null);
      // Stopped before the reset, or a frame still holding the last count
      // would land after it and leave the next run starting from the old
      // numbers.
      seedTicker.stop();
      setSeedProgress(null);
      setSeedCancelling(false);
    }
  };

  const cancelSeed = () => {
    setSeedCancelling(true);
    void invoke("cancel_seed").catch(() => {});
  };

  /// A new sign-in for a cloud copy whose old one stopped working: access
  /// removed in the account, a password change, or months unused. Only the
  /// same account is accepted, or the copy would point at an empty folder.
  const reconnect = async (id: string, kind: CloudKind) => {
    setError(null);
    setNote(null);
    setWorking(true);
    try {
      const signIn = await invoke<CloudSignIn>("cloud_sign_in", { kind });
      await invoke("backup_target_reconnect", { id, signIn: signIn.id });
      setNote(t("backup.signed_in_again", { provider: CLOUD_NAME[kind] }));
      await refresh();
      onActivity();
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setWorking(false);
    }
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
        {t("backup.copies_title")}
      </h3>
      <p>
        {t("backup.copies_aim")}{" "}
        {t("backup.copies_has", {
          current,
          count: places,
          kinds: t("backup.copies_kinds", { count: media }),
        })}
        {/* Only storage reached over the network is known to be elsewhere. A
            folder may be an external drive or a spot on the same disk. */}
        {list.some((target) => target.config.kind !== "folder")
          ? ` ${t("backup.copies_offsite")}`
          : ""}
      </p>

      <ul className="key-list copies-list">
        <li className="key-list-item">
          <span className="copy-icon" aria-hidden>
            <Laptop size={16} />
          </span>
          <div className="protected-row-text">
            <strong>{t("backup.this_computer")}</strong>
            <span className="hint">
              {fullCopy ? t("backup.this_computer_full") : t("backup.this_computer_index")}
            </span>
          </div>
        </li>

        {list.map((target) => {
          // A copy being filled is being written to, whatever the last
          // pass said; the fill records it as written when it ends.
          const state =
            seeding === target.id
              ? {
                  health: "behind" as const,
                  headline: t("backup.copy_filling"),
                  detail: t("backup.copy_filling_detail"),
                }
              : copyState(target, now);
          return (
            <li key={target.id} className="key-list-item">
              <span className={`copy-icon is-${state.health}`} aria-hidden>
                <HardDrive size={16} />
              </span>
              <div className="protected-row-text">
                <strong>
                  {target.label || whereIs(target.config)}
                  {target.primary && <span className="copy-tag">{t("backup.tag_main")}</span>}
                  {target.archive && (
                    <span className="copy-tag">{t("backup.tag_never_deletes")}</span>
                  )}
                </strong>
                {isCloudView(target.config) && (
                  <span className="hint">{target.config.account}</span>
                )}
                <span className={`hint copy-state is-${state.health}`}>{state.headline}</span>
                <span className="hint">{state.detail}</span>
                {target.archive && (
                  <span className="hint">{t("backup.archive_hint")}</span>
                )}
              </div>
              {/* The main copy is the one the card above edits. Removing it
                  there is called Disconnect and clears everything, so a
                  second way to do it here would mean two buttons with
                  different consequences. */}
              {(!target.primary || (isCloudKind(target.config.kind) && state.health !== "current")) && (
                <div className="key-list-actions">
                  {/* Only where the copy is behind: a sign-in that stopped
                      working shows as a copy that stopped being written. */}
                  {isCloudKind(target.config.kind) && state.health !== "current" && (
                    <button
                      type="button"
                      className="btn-secondary"
                      disabled={busy || working || seeding !== null}
                      onClick={() => void reconnect(target.id, target.config.kind as CloudKind)}
                      data-tooltip={t("backup.sign_in_again_title")}
                    >
                      <LogIn size={14} />
                      {t("backup.sign_in_again")}
                    </button>
                  )}
                  {!target.primary && (
                    <>
                      <button
                        type="button"
                        className="btn-secondary"
                        disabled={busy || working || seeding !== null}
                        onClick={() => void seed(target.id)}
                        data-tooltip={t("backup.fill_title")}
                      >
                        {seeding === target.id ? (
                          <span className="spinner" aria-hidden />
                        ) : (
                          <Truck size={14} />
                        )}
                        {seeding === target.id
                          ? seedProgress
                            ? seedLabel(seedProgress)
                            : t("backup.copying")
                          : t("backup.fill")}
                      </button>
                      {seeding === target.id && (
                        <button
                          type="button"
                          className="btn-secondary"
                          disabled={seedCancelling}
                          onClick={cancelSeed}
                          data-tooltip={t("backup.fill_stop_title")}
                        >
                          <X size={14} />
                          {seedCancelling ? t("backup.stopping") : t("backup.stop")}
                        </button>
                      )}
                      <button
                        type="button"
                        className="btn-secondary"
                        disabled={busy || working || seeding !== null}
                        onClick={() => setConfirmRemove(target)}
                        data-tooltip={t("backup.remove_title")}
                      >
                        <Trash2 size={14} />
                        {t("backup.remove")}
                      </button>
                    </>
                  )}
                </div>
              )}
            </li>
          );
        })}
      </ul>

      {seeding !== null && seedProgress && seedProgress.objects_total > 0 && (
        <div className="progress-row" role="status">
          <p className="hint">{seedHeadline(seedProgress)}</p>
          <div className="progress-track">
            <div className="progress-fill" style={{ width: `${seedPercent(seedProgress)}%` }} />
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
        <p className="hint">{t("backup.fill_hint")}</p>
      )}

      {adding ? (
        <div className="copies-add">
          <label className="field">
            <span>{t("backup.copy_name")}</span>
            <input
              value={label}
              disabled={working}
              placeholder={t("backup.copy_name_placeholder")}
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
              {t("backup.never_delete")}
              <span className="hint">{t("backup.never_delete_hint")}</span>
            </span>
          </label>

          {protectionWarning(protection, archive) && (
            <p className="hint is-error" role="status">
              {protectionWarning(protection, archive)}
            </p>
          )}

          <p className="hint">{t("backup.test_file_hint")}</p>
          <div className="actions">
            <button
              className="btn-primary"
              type="button"
              disabled={working}
              onClick={() => void add()}
            >
              {working ? <span className="spinner" aria-hidden /> : <Plus size={15} />}
              {working ? t("backup.checking") : t("backup.add_copy")}
            </button>
            <button
              type="button"
              className="btn-secondary"
              disabled={working}
              onClick={() => {
                discardSignIns(draft);
                setError(null);
                setAdding(false);
              }}
            >
              <X size={15} />
              {t("common.cancel")}
            </button>
          </div>
        </div>
      ) : (
        <div className="actions">
          <button
            className="btn-primary"
            type="button"
            disabled={busy || working}
            onClick={() => setAdding(true)}
          >
            <Plus size={15} />
            {t("backup.add_another")}
          </button>
        </div>
      )}

      {confirmRemove && (
        <ConfirmDialog
          title={t("backup.remove_confirm_title")}
          message={t("backup.remove_confirm_body", {
            name: confirmRemove.label || whereIs(confirmRemove.config),
          })}
          confirmLabel={t("backup.remove")}
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
