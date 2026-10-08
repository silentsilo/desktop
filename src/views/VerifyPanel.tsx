import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, CheckCircle2, LifeBuoy, SearchCheck, X } from "lucide-react";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { useLasting } from "../lib/lasting";
import { formatAppError } from "../lib/errors";
import { formatBytes } from "../lib/format";
import { markDone } from "../lib/siloMemory";
import { describeRestoreDifference } from "../lib/restoreDiff";
import { isComplete } from "../lib/recoveryCode";
import { RecoveryCodeInput } from "../components/RecoveryCodeInput";
import { t, useLocale } from "../i18n";

type Result = {
  id: string;
  label: string;
  records_read: number;
  blobs_checked: number;
  bytes_read: number;
  missing: number;
  damaged: string[];
  unreferenced: number;
  /** Put back from another copy or this computer, each with where from. */
  repaired: string[];
  /** Never rewritten: what is wrong there is only reported. */
  never_delete: boolean;
  failed: string | null;
};

/** Sound, broken, or never actually looked at. The third is not the first. */
function verdict(r: Result): "sound" | "broken" | "unchecked" {
  if (r.failed) return "unchecked";
  return r.missing > 0 || r.damaged.length > 0 ? "broken" : "sound";
}

type RestoreTest = {
  matches: boolean;
  records: number;
  entries: number;
  differences: string[];
  checked_file: string | null;
  content_error: string | null;
};

type Props = {
  busy: boolean;
  /** For remembering when the backup was last tested, for the overview. */
  siloId: string;
};

/**
 * Checking a silo against what its storage actually holds.
 *
 * The operation an archive is not credible without. A provider that quietly
 * lost an object, an upload that stopped half way, a bit that rotted: each of
 * them looks exactly like a healthy backup until the day something is
 * restored, and this is the only thing that asks before that day.
 */
const never = () => false;

export function VerifyPanel({ busy, siloId }: Props) {
  useLocale();
  // Lasting: a deep check runs for hours, and leaving the page must not
  // lose it or its answer. See lib/lasting.
  const [results, setResults] = useLasting<Result[] | null>("verify.results", null, never);
  const [error, setError] = useLasting<string | null>("verify.error", null, never);
  /// Set when the user pressed stop. Its own state rather than an error:
  /// nothing failed, and saying so in red would claim otherwise.
  const [stopped, setStopped] = useLasting<boolean>("verify.stopped", false, never);
  const [running, setRunning] = useLasting<"quick" | "deep" | null>("verify.running", null, Boolean);
  const [cancelling, setCancelling] = useLasting<boolean>("verify.cancelling", false, Boolean);
  const [progress, setProgress] = useLasting<[string, number, number] | null>(
    "verify.progress",
    null,
    Boolean,
  );
  /// The trial restore is a separate question with its own answer, so it
  /// keeps its own state rather than sharing the scrub's.
  const [code, setCode] = useState("");
  const [restore, setRestore] = useLasting<RestoreTest | null>("restore.result", null, never);
  const [restoreError, setRestoreError] = useLasting<string | null>("restore.error", null, never);
  const [restoring, setRestoring] = useLasting<boolean>("restore.running", false, Boolean);
  /// How far through the download the trial restore is. Fetching the history
  /// is the long half, and it used to run behind a disabled button alone.
  const [restoreProgress, setRestoreProgress] = useLasting<[number, number] | null>(
    "restore.progress",
    null,
    Boolean,
  );

  useEventSubscription(
    () =>
      listen<[string, number, number]>("verify-progress", (e) => setProgress(e.payload)),
    [],
  );

  useEventSubscription(
    () =>
      listen<[number, number]>("restore-progress", (e) => setRestoreProgress(e.payload)),
    [],
  );

  const runRestore = async () => {
    setRestoreError(null);
    setRestore(null);
    setRestoring(true);
    setRestoreProgress(null);
    try {
      const result = await invoke<RestoreTest>("vault_test_restore", { code });
      setRestore(result);
      if (result.matches && !result.content_error) markDone(siloId, "restore-tested");
    } catch (e) {
      setRestoreError(formatAppError(e));
    } finally {
      setRestoring(false);
      setRestoreProgress(null);
      // The code has done its job. Left in the field, it stayed on screen
      // for anyone looking or sharing the screen until the page was left.
      setCode("");
    }
  };

  const run = async (deep: boolean) => {
    setError(null);
    setStopped(false);
    setResults(null);
    setCancelling(false);
    setRunning(deep ? "deep" : "quick");
    try {
      setResults(await invoke<Result[]>("vault_verify", { deep }));
      markDone(siloId, "verified");
    } catch (e) {
      // Compared raw rather than after formatAppError, which rewrites
      // anything containing "cancelled" into a FIDO-prompt message.
      if (String(e) === "cancelled") {
        setStopped(true);
      } else {
        setError(formatAppError(e));
      }
    } finally {
      setRunning(null);
      setCancelling(false);
      setProgress(null);
    }
  };

  const cancelRun = () => {
    setCancelling(true);
    void invoke("cancel_verify").catch(() => {});
  };

  return (
    <>
    <div className="panel-section">
      <h3>
        <SearchCheck size={16} />
        {t("backup.verify_title")}
      </h3>
      <p>{t("backup.verify_intro")}</p>

      <div className="actions">
        <button
          className="btn-primary"
          type="button"
          disabled={busy || running !== null}
          onClick={() => void run(false)}
        >
          {running === "quick" ? <span className="spinner" aria-hidden /> : <SearchCheck size={15} />}
          {running === "quick" ? t("backup.checking") : t("backup.quick_check")}
        </button>
        <button
          type="button"
          className="btn-secondary"
          disabled={busy || running !== null}
          onClick={() => void run(true)}
        >
          {running === "deep" && <span className="spinner" aria-hidden />}
          {running === "deep" ? t("backup.reading_all") : t("backup.deep_check")}
        </button>
        {running !== null && (
          <button type="button" className="btn-secondary" disabled={cancelling} onClick={cancelRun}>
            <X size={15} />
            {cancelling ? t("backup.stopping") : t("backup.stop")}
          </button>
        )}
      </div>

      <p className="hint">{t("backup.verify_hint")}</p>

      {running !== null && progress && progress[2] > 0 && (
        <div className="progress-row" role="status">
          <p className="hint">
            {t("backup.verify_progress", {
              label: progress[0],
              done: progress[1],
              total: progress[2],
            })}
          </p>
          <div className="progress-track">
            <div
              className="progress-fill"
              style={{ width: `${Math.min(100, (progress[1] / progress[2]) * 100)}%` }}
            />
          </div>
        </div>
      )}

      {stopped && (
        <p className="hint" role="status">
          {t("backup.verify_stopped")}
        </p>
      )}

      {error && (
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          {error}
        </p>
      )}

      {results && (
        <ul className="key-list">
          {results.map((r) => {
            const state = verdict(r);
            return (
              <li key={r.id} className="key-list-item">
                <div className="protected-row-text">
                  <strong>{r.label || r.id}</strong>
                  {state === "sound" && (
                    <span className="hint success-msg">
                      <CheckCircle2 size={14} />
                      {r.bytes_read > 0
                        ? t("backup.verify_sound_bytes", {
                            changes: r.records_read,
                            files: r.blobs_checked,
                            size: formatBytes(r.bytes_read),
                          })
                        : t("backup.verify_sound", {
                            changes: r.records_read,
                            files: r.blobs_checked,
                          })}
                    </span>
                  )}
                  {state === "unchecked" && (
                    <span className="hint is-error">
                      {t("backup.verify_failed", { reason: r.failed ?? "" })}
                    </span>
                  )}
                  {state === "broken" && (
                    <>
                      <span className="hint is-error">
                        <AlertTriangle size={14} />
                        {r.missing > 0 ? t("backup.verify_missing", { count: r.missing }) : ""}{" "}
                        {r.damaged.length > 0
                          ? t("backup.verify_damaged", { count: r.damaged.length })
                          : ""}
                      </span>
                      {/* Named, not counted. A report that says "3 problems"
                          leaves someone with nothing to act on. */}
                      {r.damaged.slice(0, 5).map((d) => (
                        <span key={d} className="hint">
                          {d}
                        </span>
                      ))}
                      {r.damaged.length > 5 && (
                        <span className="hint">
                          {t("backup.and_more", { count: r.damaged.length - 5 })}
                        </span>
                      )}
                    </>
                  )}
                  {r.repaired.length > 0 && (
                    <>
                      <span className="hint success-msg">
                        <CheckCircle2 size={14} />
                        {t("backup.verify_repaired", { count: r.repaired.length })}
                      </span>
                      {r.repaired.slice(0, 5).map((d) => (
                        <span key={d} className="hint">
                          {d}
                        </span>
                      ))}
                      {r.repaired.length > 5 && (
                        <span className="hint">
                          {t("backup.and_more", { count: r.repaired.length - 5 })}
                        </span>
                      )}
                    </>
                  )}
                  {state === "broken" && r.never_delete && (
                    <span className="hint">{t("backup.verify_never_delete")}</span>
                  )}
                  {state === "broken" && !r.never_delete && (
                    <span className="hint">{t("backup.verify_unrepaired")}</span>
                  )}
                  {r.unreferenced > 0 && (
                    <span className="hint">
                      {t("backup.verify_leftover", { count: r.unreferenced })}
                    </span>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {results && results.every((r) => verdict(r) === "sound") && (
        <p className="hint">{t("backup.verify_again_later")}</p>
      )}

    </div>

    <div className="panel-section">
      <h3>
        <LifeBuoy size={16} />
        {t("backup.restore_title")}
      </h3>
      <p>{t("backup.restore_intro")}</p>
      <p className="hint">{t("backup.restore_hint")}</p>

      <div className="field">
        <span>{t("backup.restore_code_label")}</span>
        <RecoveryCodeInput
          value={code}
          disabled={busy || restoring}
          onChange={(next) => {
            setCode(next);
            setRestore(null);
            setRestoreError(null);
          }}
        />
      </div>

      <div className="actions">
        <button
          className="btn-primary"
          type="button"
          disabled={busy || restoring || !isComplete(code)}
          onClick={() => void runRestore()}
        >
          {restoring ? <span className="spinner" aria-hidden /> : <LifeBuoy size={15} />}
          {restoring ? t("backup.restore_running") : t("backup.restore_run")}
        </button>
      </div>

      {restoring && (
        <div className="progress-row" role="status">
          <p className="hint">
            {restoreProgress && restoreProgress[1] > 0
              ? t("backup.restore_downloading", {
                  done: restoreProgress[0],
                  total: restoreProgress[1],
                })
              : t("backup.restore_reading")}
          </p>
          {restoreProgress && restoreProgress[1] > 0 && (
            <div className="progress-track">
              <div
                className="progress-fill"
                style={{
                  width: `${Math.min(100, (restoreProgress[0] / restoreProgress[1]) * 100)}%`,
                }}
              />
            </div>
          )}
        </div>
      )}

      {restoreError && (
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          {restoreError}
        </p>
      )}

      {restore?.matches && (
        <p className="hint success-msg" role="status">
          <CheckCircle2 size={14} />
          {restore.checked_file
            ? t("backup.restore_ok_file", { count: restore.entries, file: restore.checked_file })
            : t("backup.restore_ok", { count: restore.entries })}
        </p>
      )}

      {restore && !restore.matches && (
        <>
          <p className="hint is-error" role="status">
            <AlertTriangle size={14} />
            {t("backup.restore_mismatch")}
          </p>
          {restore.content_error && (
            <p className="hint is-error">
              {restore.checked_file
                ? t("backup.restore_content_error", {
                    file: restore.checked_file,
                    error: restore.content_error,
                  })
                : restore.content_error}
            </p>
          )}
          {restore.differences.slice(0, 8).map((d) => (
            <p key={d} className="hint">
              {describeRestoreDifference(d)}
            </p>
          ))}
          {restore.differences.length > 8 && (
            <p className="hint">
              {t("backup.restore_more_differences", { count: restore.differences.length - 8 })}
            </p>
          )}
          <p className="hint">{t("backup.restore_usual_cause")}</p>
        </>
      )}
    </div>
    </>
  );
}
