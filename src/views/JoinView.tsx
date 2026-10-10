import { useEffect, useState } from "react";
import { isComplete } from "../lib/recoveryCode";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertCircle, ArrowLeft, CheckCircle2, FolderOpen, LifeBuoy } from "lucide-react";
import { open as openDialog } from "../lib/dialog";
import { AuthShell } from "../layout/AuthShell";
import { RecoveryCodeInput } from "../components/RecoveryCodeInput";
import { formatAppError } from "../lib/errors";
import { t, useLocale } from "../i18n";
import { isCloudKind } from "../lib/types";
import {
  discardSignIns,
  EMPTY_STORE_DRAFT,
  missingStoreFields,
  StoreConfigForm,
  storeDraftPayload,
  type StoreDraft,
} from "./StoreConfigForm";
import { useLasting } from "../lib/lasting";

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
    const url = draft.dav.preset === "kdrive" ? draft.dav.kdriveFolder : draft.dav.url;
    return url.split("/").filter(Boolean).pop() ?? "";
  }
  if (draft.kind === "sftp") {
    return draft.sftp.path.split("/").filter(Boolean).pop() ?? draft.sftp.host;
  }
  if (isCloudKind(draft.kind)) {
    return draft.cloud[draft.kind].folder;
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
const never = () => false;

export function JoinView({ busy, onBack, onJoined }: Props) {
  useLocale();
  const [draft, setDraft] = useState<StoreDraft>(EMPTY_STORE_DRAFT);
  const [preview, setPreview] = useState<JoinPreview | null>(null);
  // Lasting: a join is minutes of download and goes on when the settings
  // gear takes this screen away. The storage details are not kept: they
  // hold secrets. See lib/lasting.
  const [error, setError] = useLasting<string | null>("join.error", null, never);
  const [working, setWorking] = useLasting<boolean>("join.working", false, Boolean);
  const [mode, setMode] = useState<Mode>("key");
  const [code, setCode] = useState("");
  const [name, setName] = useState("");
  const [location, setLocation] = useState<string | null>(null);
  /// How far through the download the join is. Joining a large silo is
  /// minutes of work, and a spinner that says nothing for minutes reads as a
  /// hang rather than as progress.
  const [fetched, setFetched] = useLasting<{ done: number; total: number; building: boolean } | null>(
    "join.fetched",
    null,
    Boolean,
  );
  useEffect(() => {
    const stop = listen<{ fetched: number; total: number; building: boolean }>("join-progress", (event) => {
      setFetched({ done: event.payload.fetched, total: event.payload.total, building: event.payload.building });
    });
    return () => {
      void stop.then((off) => off());
    };
  }, [setFetched]);

  const handleLook = async () => {
    // Nothing is stored yet on this computer, so a blank password has
    // nothing to fall back on.
    const missing = missingStoreFields(draft, false);
    if (missing.length > 0) {
      setError(t("start.still_needed", { fields: missing.join(", ") }));
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
    <AuthShell subtitle={t("start.join_subtitle")}>
      <section className="card auth-card is-form">
        <h2>{t("welcome.join")}</h2>
        <p className="hint">{t("start.join_intro")}</p>

        <div className="s3-form">
          <StoreConfigForm
            draft={draft}
            onChange={(next) => {
              // What was found belongs to the place that was looked at.
              // Kept after an edit, it offered to set up from a place the
              // user had since changed, and joined whatever the form said.
              setDraft(next);
              setPreview(null);
            }}
            hasStoredSecret={false}
            busy={disabled}
            joining
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
        {working && fetched !== null && (fetched.building || fetched.total > 0) && (
          <p className="hint" role="status">
            {fetched.building
              ? t("start.join_building")
              : t("start.join_downloading", { done: fetched.done, total: fetched.total })}
          </p>
        )}

        {preview && !preview.vault_id && (
          <p className="hint is-error" role="status">
            <AlertCircle size={14} />
            {t("start.join_no_silo")}
          </p>
        )}

        {preview?.vault_id && (
          <>
            <label className="field">
              <span>{t("start.join_name_label")}</span>
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={defaultSiloName(draft) || t("start.default_name")}
              />
            </label>
            <label className="field">
              <span>{t("start.join_location_label")}</span>
              <div className="path-picker">
                <input
                  value={location ?? ""}
                  onChange={(e) => setLocation(e.target.value || null)}
                  placeholder={t("start.join_default_location")}
                  spellCheck={false}
                />
                <button
                  type="button"
                  className="btn-secondary"
                  disabled={disabled}
                  onClick={() => {
                    void openDialog({ directory: true, multiple: false }).then((picked) => {
                      if (typeof picked === "string") setLocation(picked);
                    });
                  }}
                >
                  <FolderOpen size={15} />
                  {t("start.browse")}
                </button>
              </div>
            </label>
            <p className="hint success-msg" role="status">
              <CheckCircle2 size={14} />
              {preview.key_labels.length > 0
                ? t("start.join_found_keys", { keys: preview.key_labels.join(", ") })
                : t("start.join_found_no_keys")}
            </p>
            {mode === "code" ? (
              <div className="field">
                <span>{t("settings.recovery")}</span>
                <RecoveryCodeInput value={code} onChange={setCode} disabled={disabled} />
                <p className="hint">{t("start.join_code_hint")}</p>
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
                  {t("start.join_use_code")}
                </button>
              </p>
            )}
          </>
        )}

        <div className="actions">
          {preview?.vault_id &&
          (mode === "code" || preview.key_labels.length > 0) ? (
            <button
              className="btn-primary"
              type="button"
              disabled={disabled || (mode === "code" && !isComplete(code))}
              onClick={() => void handleJoin()}
            >
              {working && <span className="spinner" aria-hidden />}
              {working
                ? mode === "code"
                  ? t("start.join_setting_up")
                  : t("start.join_waiting_key")
                : t("start.join_set_up")}
            </button>
          ) : (
            <button
              className="btn-primary"
              type="button"
              disabled={disabled}
              onClick={() => void handleLook()}
            >
              {working ? t("start.checking") : t("start.join_look")}
            </button>
          )}
          <button
            type="button"
            className="btn-secondary"
            disabled={disabled}
            onClick={() => {
              discardSignIns(draft);
              onBack();
            }}
          >
            <ArrowLeft size={15} />
            {t("start.back")}
          </button>
        </div>
      </section>
    </AuthShell>
  );
}
