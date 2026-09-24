import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, CheckCircle2, LifeBuoy, SearchCheck, X } from "lucide-react";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { formatAppError } from "../lib/errors";
import { formatBytes } from "../lib/format";
import { markDone } from "../lib/siloMemory";
import { describeRestoreDifference } from "../lib/restoreDiff";
import { isComplete } from "../lib/recoveryCode";
import { RecoveryCodeInput } from "../components/RecoveryCodeInput";

type Result = {
  id: string;
  label: string;
  records_read: number;
  blobs_checked: number;
  bytes_read: number;
  missing: number;
  damaged: string[];
  unreferenced: number;
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
export function VerifyPanel({ busy, siloId }: Props) {
  const [results, setResults] = useState<Result[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  /// Set when the user pressed stop. Its own state rather than an error:
  /// nothing failed, and saying so in red would claim otherwise.
  const [stopped, setStopped] = useState(false);
  const [running, setRunning] = useState<"quick" | "deep" | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [progress, setProgress] = useState<[string, number, number] | null>(null);
  /// The trial restore is a separate question with its own answer, so it
  /// keeps its own state rather than sharing the scrub's.
  const [code, setCode] = useState("");
  const [restore, setRestore] = useState<RestoreTest | null>(null);
  const [restoreError, setRestoreError] = useState<string | null>(null);
  const [restoring, setRestoring] = useState(false);
  /// How far through the download the trial restore is. Fetching the history
  /// is the long half, and it used to run behind a disabled button alone.
  const [restoreProgress, setRestoreProgress] = useState<[number, number] | null>(null);

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
    <div className="panel-section">
      <h3>
        <SearchCheck size={16} />
        Check this silo against its backup storage
      </h3>
      <p>
        Reads what each copy holds and compares it with what this silo expects. Lost or damaged
        files do not show up any other way until you try to restore them.
      </p>

      <div className="actions">
        <button type="button" disabled={busy || running !== null} onClick={() => void run(false)}>
          {running === "quick" ? <span className="spinner" aria-hidden /> : <SearchCheck size={15} />}
          {running === "quick" ? "Checking…" : "Quick check"}
        </button>
        <button
          type="button"
          className="secondary"
          disabled={busy || running !== null}
          onClick={() => void run(true)}
        >
          {running === "deep" && <span className="spinner" aria-hidden />}
          {running === "deep" ? "Reading everything…" : "Read every file back"}
        </button>
        {running !== null && (
          <button type="button" className="secondary" disabled={cancelling} onClick={cancelRun}>
            <X size={15} />
            {cancelling ? "Stopping…" : "Stop"}
          </button>
        )}
      </div>

      <p className="hint">
        The quick check looks for missing or empty files and is fast at any size. Reading every
        file back also finds damaged ones, but downloads the whole silo. Stopping changes nothing.
      </p>

      {running !== null && progress && progress[2] > 0 && (
        <div className="progress-row" role="status">
          <p className="hint">
            {progress[0]}: {progress[1]} of {progress[2]}
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
          Stopped. Nothing was changed.
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
                      No problems found. {r.records_read} changes and {r.blobs_checked} files
                      checked
                      {r.bytes_read > 0 ? `, ${formatBytes(r.bytes_read)} read back` : ""}.
                    </span>
                  )}
                  {state === "unchecked" && (
                    <span className="hint is-error">
                      Could not be checked: {r.failed}
                    </span>
                  )}
                  {state === "broken" && (
                    <>
                      <span className="hint is-error">
                        <AlertTriangle size={14} />
                        {r.missing > 0
                          ? r.missing === 1
                            ? "1 file this silo believes it has is not there."
                            : `${r.missing} files this silo believes it has are not there.`
                          : ""}{" "}
                        {r.damaged.length > 0
                          ? `${r.damaged.length} file${r.damaged.length === 1 ? " is" : "s are"} damaged.`
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
                        <span className="hint">and {r.damaged.length - 5} more.</span>
                      )}
                    </>
                  )}
                  {r.unreferenced > 0 && (
                    <span className="hint">
                      {r.unreferenced} leftover file{r.unreferenced === 1 ? "" : "s"} this silo
                      does not use. This is normal: they are cleared up later.
                    </span>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {results && results.every((r) => verdict(r) === "sound") && (
        <p className="hint">
          A check covers only the moment it ran. Run it again every few months.
        </p>
      )}

      <h3 className="verify-restore-head">
        <LifeBuoy size={16} />
        Test a recovery
      </h3>
      <p>
        Rebuilds this silo in a temporary folder from backup storage and your recovery code, then
        compares it with what you have. This is how you would get the silo back if this computer
        were lost.
      </p>
      <p className="hint">
        It rebuilds the file list and opens one file, to show the code still opens your content.
        Nothing here is changed, and the rebuild is deleted afterwards.
      </p>

      <div className="field">
        <span>Your recovery code</span>
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
          type="button"
          disabled={busy || restoring || !isComplete(code)}
          onClick={() => void runRestore()}
        >
          {restoring ? <span className="spinner" aria-hidden /> : <LifeBuoy size={15} />}
          {restoring ? "Rebuilding…" : "Try a recovery now"}
        </button>
      </div>

      {restoring && (
        <div className="progress-row" role="status">
          <p className="hint">
            {restoreProgress && restoreProgress[1] > 0
              ? `Downloading this silo's changes: ${restoreProgress[0]} of ${restoreProgress[1]}.`
              : "Reading the backup storage…"}
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
          Recovery works. Your code and backup storage rebuilt {restore.entries} folders and files
          {restore.checked_file ? `, and “${restore.checked_file}” opened and matched` : ""}.
        </p>
      )}

      {restore && !restore.matches && (
        <>
          <p className="hint is-error" role="status">
            <AlertTriangle size={14} />
            The rebuild does not match what you have. Your files here are fine, but recovering from
            backup storage now would not give you the same silo.
          </p>
          {restore.content_error && (
            <p className="hint is-error">
              {restore.checked_file
                ? `“${restore.checked_file}” could not be opened from backup storage: ${restore.content_error}`
                : restore.content_error}
            </p>
          )}
          {restore.differences.slice(0, 8).map((d) => (
            <p key={d} className="hint">
              {describeRestoreDifference(d)}
            </p>
          ))}
          {restore.differences.length > 8 && (
            <p className="hint">and {restore.differences.length - 8} more differences.</p>
          )}
          <p className="hint">
            The usual cause is changes that have not reached backup storage yet. Sync, then try again.
          </p>
        </>
      )}
    </div>
  );
}
