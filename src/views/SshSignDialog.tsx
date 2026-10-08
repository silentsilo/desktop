import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { KeyRound } from "lucide-react";
import type { Os, SshSignPrompt } from "../lib/types";
import { builtInOrKey, platformStrings } from "../lib/platformStrings";
import { formatAppError } from "../lib/errors";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { useModal } from "../hooks/useModal";
import { t, useLocale } from "../i18n";

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
  if (prompt.namespace === "git") return t("dlg.ssh_purpose_git");
  if (prompt.namespace) return t("dlg.ssh_purpose_namespace", { namespace: prompt.namespace });
  if (prompt.user) return t("dlg.ssh_purpose_user", { user: prompt.user });
  return t("dlg.ssh_purpose_server");
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
  useLocale();
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

  const who = program(prompt.program);
  return (
    <div className="modal-overlay" onClick={busy ? undefined : cancel}>
      <div
        ref={cardRef}
        className="modal-card browser-fill"
        role="alertdialog"
        aria-modal="true"
        aria-label={t("dlg.ssh_title")}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-title-row">
          <span className="modal-title-icon" aria-hidden>
            <KeyRound size={18} />
          </span>
          <h3 className="modal-title">{t("dlg.ssh_title")}</h3>
        </div>
        <div className="modal-body">
          {/* The program is what the system reports. Any program running as
              you can ask, so the name is a hint, not a proof. */}
          <p>
            {who
              ? t("dlg.ssh_body", { program: who, key: prompt.key })
              : t("dlg.ssh_body_unknown", { key: prompt.key })}
          </p>
          <dl className="browser-fill-facts">
            <dt>{t("dlg.ssh_for")}</dt>
            <dd>{purpose(prompt)}</dd>
            <dt>{t("dlg.ssh_server")}</dt>
            <dd>
              {prompt.host ??
                (prompt.namespace ? t("dlg.ssh_server_none") : t("dlg.ssh_server_unnamed"))}
            </dd>
            <dt>{t("dlg.ssh_program")}</dt>
            <dd>
              {prompt.program ?? t("dlg.ssh_program_unknown")}
              {prompt.parent && (
                <span className="hint">
                  {" "}
                  {t("dlg.ssh_started_by", { parent: program(prompt.parent) ?? prompt.parent })}
                </span>
              )}
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
                  ? t("dlg.ssh_remember_git")
                  : t("dlg.ssh_remember_server")}
              </span>
            </label>
          )}
          {prompt.require_reauth && (
            <p className="hint">
              {t("dlg.ssh_confirm_next", { method: builtInOrKey(platform) })}
            </p>
          )}
          {error && <p className="hint is-error">{error}</p>}
        </div>
        <div className="modal-actions">
          <button type="button" className="btn-secondary" disabled={busy} onClick={cancel}>
            {t("common.cancel")}
          </button>
          <button
            className="btn-primary"
            type="button"
            disabled={busy || !armed}
            onClick={() => void confirm()}
          >
            {t("dlg.ssh_sign")}
          </button>
        </div>
      </div>
    </div>
  );
}
