import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { KeyRound } from "lucide-react";
import type { Os, SshSignPrompt } from "../lib/types";
import { platformStrings } from "../lib/platformStrings";
import { formatAppError } from "../lib/errors";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { useModal } from "../hooks/useModal";

/**
 * A signature an SSH client asked the agent for, decided here. The Rust
 * side brings the window forward, sends `ssh-sign-request`, and closes the
 * question with `ssh-sign-ended` however it ends. The key never passes
 * through here.
 */
/// Long enough to outlast a click in flight, as for a fill.
const ARM_DELAY_MS = 700;

export function SshSignDialog({ os }: { os: Os }) {
  const [prompt, setPrompt] = useState<SshSignPrompt | null>(null);

  useEffect(() => {
    let cancelled = false;
    invoke<SshSignPrompt | null>("ssh_sign_pending")
      .then((pending) => {
        if (!cancelled && pending) setPrompt(pending);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  useEventSubscription(
    () => listen<SshSignPrompt>("ssh-sign-request", (event) => setPrompt(event.payload)),
    [],
  );
  useEventSubscription(
    () =>
      listen<string>("ssh-sign-ended", (event) =>
        setPrompt((current) => (current?.request_id === event.payload ? null : current)),
      ),
    [],
  );

  if (!prompt) return null;
  return (
    <SignCard key={prompt.request_id} prompt={prompt} os={os} onClose={() => setPrompt(null)} />
  );
}

/** The file name of a path, which is what a person recognises. */
function program(path: string | null): string | null {
  if (!path) return null;
  return path.split(/[\\/]/).pop() || path;
}

/** What the signature is for, in words. */
function purpose(prompt: SshSignPrompt): string {
  if (prompt.namespace === "git") return "Sign a Git commit or tag";
  if (prompt.namespace) return `Sign data for "${prompt.namespace}"`;
  if (prompt.user) return `Sign in as ${prompt.user}`;
  return "Sign in to a server";
}

function SignCard({
  prompt,
  os,
  onClose,
}: {
  prompt: SshSignPrompt;
  os: Os;
  onClose: () => void;
}) {
  const platform = platformStrings(os);
  const [busy, setBusy] = useState(false);
  const [remember, setRemember] = useState(false);
  const [armed, setArmed] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const timer = window.setTimeout(() => setArmed(true), ARM_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, []);

  const cancel = () => {
    void invoke("ssh_sign_cancel", { requestId: prompt.request_id }).catch(() => {});
    onClose();
  };
  const cardRef = useModal(busy ? undefined : cancel);

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("ssh_sign_confirm", { requestId: prompt.request_id, remember });
      onClose();
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setBusy(false);
    }
  };

  const who = program(prompt.program) ?? "A program";
  return (
    <div className="modal-overlay" onClick={busy ? undefined : cancel}>
      <div
        ref={cardRef}
        className="modal-card browser-fill"
        role="alertdialog"
        aria-modal="true"
        aria-label="Use an SSH key?"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-title-row">
          <span className="modal-title-icon" aria-hidden>
            <KeyRound size={18} />
          </span>
          <h3 className="modal-title">Use an SSH key?</h3>
        </div>
        <div className="modal-body">
          {/* The program is what the system reports. Any program running as
              you can ask, so the name is a hint, not a proof. */}
          <p>
            {who} asks to use your {prompt.key} key. If you did not just start ssh, Git or a
            connection in your editor, choose Cancel.
          </p>
          <dl className="browser-fill-facts">
            <dt>For</dt>
            <dd>{purpose(prompt)}</dd>
            <dt>Server</dt>
            <dd>
              {prompt.host ?? (prompt.namespace ? "None" : "Not named by the program")}
            </dd>
            <dt>Program</dt>
            <dd>
              {prompt.program ?? "Unknown"}
              {prompt.parent && <span className="hint"> started by {program(prompt.parent)}</span>}
            </dd>
          </dl>
          {prompt.can_remember && (
            <label className="confirm-option">
              <input
                type="checkbox"
                checked={remember}
                disabled={busy}
                onChange={(e) => setRemember(e.target.checked)}
              />
              <span>
                {prompt.namespace === "git"
                  ? "Allow this key for Git signatures until the silo locks"
                  : "Allow this key for this server until the silo locks"}
              </span>
            </label>
          )}
          {prompt.require_reauth && (
            <p className="hint">You confirm with {platform.builtIn} or your security key next.</p>
          )}
          {error && <p className="hint is-error">{error}</p>}
        </div>
        <div className="modal-actions">
          <button type="button" className="secondary" disabled={busy} onClick={cancel}>
            Cancel
          </button>
          <button type="button" disabled={busy || !armed} onClick={() => void confirm()}>
            Sign
          </button>
        </div>
      </div>
    </div>
  );
}
