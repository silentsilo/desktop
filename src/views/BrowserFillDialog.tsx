import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Globe } from "lucide-react";
import type { BrowserFillPrompt, Os } from "../lib/types";
import { platformStrings } from "../lib/platformStrings";
import { formatAppError } from "../lib/errors";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { useModal } from "../hooks/useModal";

/**
 * The browser extension's fill, confirmed here rather than in the browser:
 * a question drawn inside the browser is drawn by what we do not trust.
 *
 * Mounted while a silo is open. The Rust side brings the window to the
 * front, sends `browser-fill-request`, and closes the question with
 * `browser-fill-ended` however it ends (answered, declined, timed out, the
 * silo locked). The password never passes through here.
 */
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
  const platform = platformStrings(os);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const cancel = () => {
    void invoke("browser_fill_cancel", { requestId: prompt.request_id }).catch(() => {});
    onClose();
  };
  const cardRef = useModal(busy ? undefined : cancel);

  useEventSubscription(
    () =>
      listen<string>("fido-progress", (event) => {
        if (busy) setProgress(event.payload);
      }),
    [busy],
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
        aria-label="Fill a login in your browser?"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-title-row">
          <span className="modal-title-icon" aria-hidden>
            <Globe size={18} />
          </span>
          <h3 className="modal-title">Fill a login in your browser?</h3>
        </div>
        <div className="modal-body">
          {/* Says only what the app knows: the request came through the
              browser channel. Another program of this user can send one too. */}
          <p>
            A fill request from your browser for {prompt.site}. If you did not just click
            SilentSilo in the browser, choose Cancel.
          </p>
          <dl className="browser-fill-facts">
            <dt>Site</dt>
            <dd>{prompt.site}</dd>
            <dt>Login</dt>
            <dd>
              {prompt.label}
              {prompt.username && <span className="hint"> {prompt.username}</span>}
            </dd>
          </dl>
          {prompt.mismatch && (
            <p className="browser-fill-mismatch" role="alert">
              {prompt.mismatch} Fill it only if you meant to use it on this site.
            </p>
          )}
          <p className="hint">
            {platform.builtIn} or your security key is asked next. The browser gets nothing
            before that.
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
            Cancel
          </button>
          <button
            type="button"
            className={prompt.mismatch ? "danger" : undefined}
            disabled={busy}
            onClick={() => void confirm()}
          >
            {prompt.mismatch ? "Fill anyway" : "Fill"}
          </button>
        </div>
      </div>
    </div>
  );
}
