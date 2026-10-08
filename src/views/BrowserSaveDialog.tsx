import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Globe } from "lucide-react";
import type { BrowserSavePrompt, EntryChange, PasswordEntry } from "../lib/types";
import { alreadySaved, existingEntry, savedEntry } from "../lib/browserSave";
import { formatAppError } from "../lib/errors";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { useModal } from "../hooks/useModal";
import { t, useLocale } from "../i18n";

/**
 * A login the browser extension read from a page on the person's click,
 * offered for saving. Nothing is written until Save here; the entry goes
 * through the same save as an edit, so an update keeps the old password in
 * the history and the activity log records it.
 *
 * The Rust side brings the window forward, sends `browser-save-request`,
 * and closes the question with `browser-save-ended` however it ends.
 */
/// Long enough to outlast a click in flight, as for a fill.
const ARM_DELAY_MS = 700;

export function BrowserSaveDialog({
  entries,
  onSave,
}: {
  entries: PasswordEntry[];
  onSave: (entry: PasswordEntry, change?: EntryChange) => Promise<boolean>;
}) {
  const [prompt, setPrompt] = useState<BrowserSavePrompt | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<BrowserSavePrompt | null>("browser_save_pending")
      .then((pending) => {
        if (!cancelled && pending) setPrompt(pending);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  useEventSubscription(
    () => listen<BrowserSavePrompt>("browser-save-request", (event) => setPrompt(event.payload)),
    [],
  );
  useEventSubscription(
    () =>
      listen<string>("browser-save-ended", (event) =>
        setPrompt((current) => (current?.request_id === event.payload ? null : current)),
      ),
    [],
  );

  if (!prompt) return null;
  return (
    <SaveCard
      key={prompt.request_id}
      prompt={prompt}
      entries={entries}
      onSave={onSave}
      onClose={() => setPrompt(null)}
    />
  );
}

function SaveCard({
  prompt,
  entries,
  onSave,
  onClose,
}: {
  prompt: BrowserSavePrompt;
  entries: PasswordEntry[];
  onSave: (entry: PasswordEntry, change?: EntryChange) => Promise<boolean>;
  onClose: () => void;
}) {
  useLocale();
  const existing = existingEntry(prompt, entries);
  const unchanged = alreadySaved(prompt, existing);
  const [update, setUpdate] = useState(existing !== undefined);
  const [label, setLabel] = useState(prompt.label);
  const [username, setUsername] = useState(prompt.username);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    const timer = window.setTimeout(() => setArmed(true), ARM_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);

  const cancel = () => {
    void invoke("browser_save_cancel", { requestId: prompt.request_id }).catch(() => {});
    onClose();
  };
  const cardRef = useModal(busy ? undefined : cancel);

  const save = async () => {
    setBusy(true);
    setError(null);
    const { entry, updated } = savedEntry(
      prompt,
      update ? existing : undefined,
      label,
      username.trim(),
      Date.now(),
    );
    try {
      if (!(await onSave(entry, updated ? "edited" : "created"))) {
        setError(t("dlg.save_failed"));
        return;
      }
      // Saved here whatever the browser hears: it may have stopped waiting.
      await invoke("browser_save_done", { requestId: prompt.request_id, updated }).catch(() => {});
      onClose();
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-overlay" onClick={busy ? undefined : cancel}>
      <div
        ref={cardRef}
        className="modal-card browser-fill"
        role="alertdialog"
        aria-modal="true"
        aria-label={t("dlg.save_title")}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-title-row">
          <span className="modal-title-icon" aria-hidden>
            <Globe size={18} />
          </span>
          <h3 className="modal-title">{t("dlg.save_title")}</h3>
        </div>
        <div className="modal-body">
          {/* As for a fill: the request came through the browser channel,
              which another program of this user can reach too. */}
          <p>{t("dlg.save_body", { site: prompt.site })}</p>
          {unchanged ? (
            <p>{t("dlg.save_unchanged", { name: existing!.service })}</p>
          ) : (
            <>
              {existing && (
                <div className="browser-save-choice" role="radiogroup">
                  <label className="key-choice">
                    <input
                      type="radio"
                      name="browser-save"
                      checked={update}
                      disabled={busy}
                      onChange={() => setUpdate(true)}
                    />
                    <span>
                      {t("dlg.save_update_option", { name: existing.service })}
                      <span className="hint"> {t("dlg.save_update_hint")}</span>
                    </span>
                  </label>
                  <label className="key-choice">
                    <input
                      type="radio"
                      name="browser-save"
                      checked={!update}
                      disabled={busy}
                      onChange={() => setUpdate(false)}
                    />
                    <span>{t("dlg.save_new_option")}</span>
                  </label>
                </div>
              )}
              {!update && (
                <label className="field">
                  <span>{t("dlg.save_name")}</span>
                  <input
                    value={label}
                    disabled={busy}
                    maxLength={200}
                    onChange={(e) => setLabel(e.target.value)}
                  />
                </label>
              )}
              <label className="field">
                <span>{t("dlg.save_username")}</span>
                <input
                  value={username}
                  disabled={busy}
                  autoComplete="off"
                  spellCheck={false}
                  onChange={(e) => setUsername(e.target.value)}
                />
              </label>
              <p className="hint">{t("dlg.save_password_hint")}</p>
            </>
          )}
          {error && <p className="hint is-error">{error}</p>}
        </div>
        <div className="modal-actions">
          <button type="button" className="btn-secondary" disabled={busy} onClick={cancel}>
            {unchanged ? t("dlg.close") : t("common.cancel")}
          </button>
          {!unchanged && (
            <button
              className="btn-primary"
              type="button"
              disabled={busy || !armed}
              onClick={() => void save()}
            >
              {update && existing ? t("dlg.update") : t("dlg.save")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
