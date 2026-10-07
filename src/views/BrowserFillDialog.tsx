import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Globe } from "lucide-react";
import type { BrowserFillPrompt, Os } from "../lib/types";
import { builtInOrKey, platformStrings } from "../lib/platformStrings";
import { describeFidoPrompt, type FidoPrompt } from "../lib/fidoPrompt";
import { formatAppError } from "../lib/errors";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { useModal } from "../hooks/useModal";
import { t, useLocale } from "../i18n";

/**
 * The browser extension's fill, confirmed here rather than in the browser:
 * a question drawn inside the browser is drawn by what we do not trust.
 *
 * Mounted while a silo is open. The Rust side brings the window to the
 * front, sends `browser-fill-request`, and closes the question with
 * `browser-fill-ended` however it ends (answered, declined, timed out, the
 * silo locked). The password never passes through here.
 */
/// Long enough to outlast a click in flight, short enough not to be noticed.
const ARM_DELAY_MS = 700;

export function BrowserFillDialog({ os }: { os: Os }) {
  const [prompt, setPrompt] = useState<BrowserFillPrompt | null>(null);

  // A request that arrived before this mounted, such as while the window
  // was still switching screens.
  useEffect(() => {
    let cancelled = false;
    invoke<BrowserFillPrompt | null>("browser_fill_pending")
      .then((pending) => {
        if (!cancelled && pending) setPrompt(pending);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  useEventSubscription(
    () => listen<BrowserFillPrompt>("browser-fill-request", (event) => setPrompt(event.payload)),
    [],
  );
  useEventSubscription(
    () =>
      listen<string>("browser-fill-ended", (event) =>
        setPrompt((current) => (current?.request_id === event.payload ? null : current)),
      ),
    [],
  );

  if (!prompt) return null;
  return (
    <FillCard
      key={prompt.request_id}
      prompt={prompt}
      os={os}
      onClose={() => setPrompt(null)}
    />
  );
}

function FillCard({
  prompt,
  os,
  onClose,
}: {
  prompt: BrowserFillPrompt;
  os: Os;
  onClose: () => void;
}) {
  useLocale();
  const platform = platformStrings(os);
  const [busy, setBusy] = useState(false);
  // Fill stays inert for a moment after a question appears. The card is
  // keyed by request, so a new question mounts a new card: a click already
  // on its way, aimed at the one before, lands on nothing.
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    const timer = window.setTimeout(() => setArmed(true), ARM_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);
  const [progress, setProgress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const cancel = () => {
    void invoke("browser_fill_cancel", { requestId: prompt.request_id }).catch(() => {});
    onClose();
  };
  const cardRef = useModal(busy ? undefined : cancel);

  useEventSubscription(
    () =>
      listen<FidoPrompt>("fido-progress", (event) => {
        if (busy) setProgress(describeFidoPrompt(event.payload, platformStrings(os).builtIn));
      }),
    [busy, os],
  );

  const confirm = async () => {
    setBusy(true);
    setError(null);
    setProgress(null);
    try {
      await invoke("browser_fill_confirm", { requestId: prompt.request_id });
      onClose();
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setBusy(false);
      setProgress(null);
    }
  };

  return (
    <div className="modal-overlay" onClick={busy ? undefined : cancel}>
      <div
        ref={cardRef}
        className="modal-card browser-fill"
        role="alertdialog"
        aria-modal="true"
        aria-label={t("dlg.fill_title")}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-title-row">
          <span className="modal-title-icon" aria-hidden>
            <Globe size={18} />
          </span>
          <h3 className="modal-title">{t("dlg.fill_title")}</h3>
        </div>
        <div className="modal-body">
          {/* Says only what the app knows: the request came through the
              browser channel. Another program of this user can send one too. */}
          <p>{t("dlg.fill_body", { site: prompt.site })}</p>
          <dl className="browser-fill-facts">
            <dt>{t("dlg.fill_site")}</dt>
            <dd>{prompt.site}</dd>
            <dt>{t("dlg.fill_login")}</dt>
            <dd>
              {prompt.label}
              {prompt.username && <span className="hint"> {prompt.username}</span>}
            </dd>
          </dl>
          {prompt.mismatch && (
            <p className="browser-fill-mismatch" role="alert">
              {prompt.mismatch} {t("dlg.fill_mismatch_advice")}
            </p>
          )}
          <p className="hint">
            {t("dlg.fill_confirm_next", { method: builtInOrKey(platform) })}
          </p>
          {busy && progress && (
            <p className="hint" role="status">
              {progress}
            </p>
          )}
          {error && <p className="hint is-error">{error}</p>}
        </div>
        <div className="modal-actions">
          <button type="button" className="secondary" disabled={busy} onClick={cancel}>
            {t("common.cancel")}
          </button>
          <button
            type="button"
            className={prompt.mismatch ? "danger" : undefined}
            disabled={busy || !armed}
            onClick={() => void confirm()}
          >
            {prompt.mismatch ? t("dlg.fill_anyway") : t("dlg.fill_button")}
          </button>
        </div>
      </div>
    </div>
  );
}
