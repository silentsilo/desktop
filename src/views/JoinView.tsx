import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, ArrowLeft, CheckCircle2, FolderOpen, LifeBuoy } from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { AuthShell } from "../layout/AuthShell";
import { RecoveryCodeInput } from "../components/RecoveryCodeInput";
import { formatAppError } from "../lib/errors";
import {
  EMPTY_STORE_DRAFT,
  missingStoreFields,
  StoreConfigForm,
  storeDraftPayload,
  type StoreDraft,
} from "./StoreConfigForm";

type JoinPreview = {
  vault_id: string | null;
  key_labels: string[];
};

type Mode = "key" | "code";

/// A sensible name suggestion, taken from wherever the backup lives.
function defaultSiloName(draft: StoreDraft): string {
  if (draft.kind === "folder") {
    // Both separators: the folder comes from the OS picker, so on Windows it
    // arrives backslash-separated and splitting on "/" alone would suggest
    // the whole path as the silo's name.
    return draft.folder.split(/[\\/]/).filter(Boolean).pop() ?? "";
  }
  if (draft.kind === "web-dav") {
    return draft.dav.url.split("/").filter(Boolean).pop() ?? "";
  }
  if (draft.kind === "sftp") {
    return draft.sftp.path.split("/").filter(Boolean).pop() ?? draft.sftp.host;
  }
  return draft.s3.bucket;
}

type Props = {
  busy: boolean;
  onBack: () => void;
  /** Runs after the silo has been joined and the session is open. */
  onJoined: (meta: unknown) => Promise<void> | void;
};

/**
 * Rebuilding a silo on this computer from what is in its backup storage.
 *
 * Deliberately not called "joining": nothing here keeps working against storage.
 * The operation log is replayed into a new local silo, and from then on
 * this computer reads and writes locally, with storage kept in step in the
 * background like every other silo.
 *
 * The storage is described first, then read to see whether it holds a silo
 * and which keys can open it — all before anything local is created, so
 * being pointed at the wrong place costs nothing but a correction.
 */
export function JoinView({ busy, onBack, onJoined }: Props) {
  const [draft, setDraft] = useState<StoreDraft>(EMPTY_STORE_DRAFT);
  const [preview, setPreview] = useState<JoinPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const [mode, setMode] = useState<Mode>("key");
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const [location, setLocation] = useState<string | null>(null);
  /// How far through the download the join is. Joining a large silo is
  /// minutes of work, and a spinner that says nothing for minutes reads as a
  /// hang rather than as progress.
  const [fetched, setFetched] = useState<{ done: number; total: number } | null>(null);
  useEffect(() => {
    const stop = listen<{ fetched: number; total: number }>("join-progress", (event) => {
      setFetched({ done: event.payload.fetched, total: event.payload.total });
    });
    return () => {
      void stop.then((off) => off());
    };
  }, []);

  const handleLook = async () => {
    // Nothing is stored yet on this computer, so a blank password has
    // nothing to fall back on.
    const missing = missingStoreFields(draft, false);
    if (missing.length > 0) {
      setError(`Still needed: ${missing.join(", ")}.`);
      return;
    }
    setWorking(true);
    setError(null);
    setPreview(null);
    try {
      // Nothing is saved yet: the storage details are passed straight to the
      // check, because there is no silo to store them in until one is
      setPreview(await invoke<JoinPreview>("vault_preview_join", { config: payload() }));
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setWorking(false);
    }
  };

  const payload = () => storeDraftPayload(draft);

  const handleJoin = async () => {
    setWorking(true);
    setError(null);
    try {
      const siloName = name.trim() || defaultSiloName(draft);
      const args =
        mode === "code"
          ? { config: payload(), code, name: siloName, location }
          : { config: payload(), name: siloName, location };
      const command =
        mode === "code" ? "vault_join_with_recovery" : "vault_join_from_storage";
      setFetched(null);
      await onJoined(await invoke(command, args));
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setWorking(false);
    }
  };

  const disabled = busy || working;

  return (
    <AuthShell
      subtitle="Bring a silo you already back up onto this computer."
    >
      <section className="card auth-card is-form">
        <h2>Set up from backup storage</h2>
        <p className="hint">
          Point this computer at wherever your silo backs up. Its contents are copied here and
          decrypted with a security key you already have. After that you work locally, exactly
          like any other silo.
        </p>

        <div className="s3-form">
          <StoreConfigForm
            draft={draft}
            onChange={setDraft}
            hasStoredSecret={false}
            busy={disabled}
          />
        </div>

        {error && (
          <p className="hint is-error" role="status">
            <AlertCircle size={14} />
            {error}
          </p>
        )}

        {/* Shown while the download runs, and only then: a count left on
            screen after it finishes is a number nobody is waiting for. */}
        {working && fetched !== null && fetched.total > 0 && (
          <p className="hint" role="status">
            Fetching this silo's history: {fetched.done} of {fetched.total} changes.
          </p>
        )}

        {preview && !preview.vault_id && (
          <p className="hint is-error" role="status">
            <AlertCircle size={14} />
            There is no silo there yet. Sync once from the computer that has it, or create a new
            silo here instead.
          </p>
        )}

        {preview?.vault_id && (
          <>
            <label className="field">
              <span>Call this silo</span>
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={defaultSiloName(draft) || "Personal"}
              />
            </label>
            <label className="field">
              <span>Where to keep it on this computer</span>
              <div className="path-picker">
                <input
                  value={location ?? ""}
                  onChange={(e) => setLocation(e.target.value || null)}
                  placeholder="Default location"
                  spellCheck={false}
                />
                <button
                  type="button"
                  className="secondary"
                  disabled={disabled}
                  onClick={() => {
                    void openDialog({ directory: true, multiple: false }).then((picked) => {
                      if (typeof picked === "string") setLocation(picked);
                    });
                  }}
                >
                  <FolderOpen size={15} />
                  Browse
                </button>
              </div>
            </label>
            <p className="hint success-msg" role="status">
              <CheckCircle2 size={14} />
              {preview.key_labels.length > 0
                ? `Found a silo. Keys that can open it: ${preview.key_labels.join(", ")}.`
                : "Found a silo, but no security keys have been published to it yet."}
            </p>
            {mode === "code" ? (
              <div className="field">
                <span>Recovery code</span>
                <RecoveryCodeInput value={code} onChange={setCode} disabled={disabled} />
                <p className="hint">
                  One group per box; paste the whole code into any of them and the rest fill
                  themselves. This rebuilds the silo here from storage alone, with no security
                  key needed.
                </p>
              </div>
            ) : (
              <p className="hint">
                <button
                  type="button"
                  className="link"
                  disabled={disabled}
                  onClick={() => setMode("code")}
                >
                  <LifeBuoy size={14} />
                  No key left? Use your recovery code instead
                </button>
              </p>
            )}
          </>
        )}

        <div className="actions">
          {preview?.vault_id &&
          (mode === "code" || preview.key_labels.length > 0) ? (
            <button
              type="button"
              disabled={disabled || (mode === "code" && code.trim().length === 0)}
              onClick={() => void handleJoin()}
            >
              {working && <span className="spinner" aria-hidden />}
              {working
                ? mode === "code"
                  ? "Restoring…"
                  : "Waiting for your key…"
                : "Set up on this computer"}
            </button>
          ) : (
            <button type="button" disabled={disabled} onClick={() => void handleLook()}>
              {working ? "Checking…" : "See what is there"}
            </button>
          )}
          <button type="button" className="secondary" disabled={disabled} onClick={onBack}>
            <ArrowLeft size={15} />
            Back
          </button>
        </div>
      </section>
    </AuthShell>
  );
}
