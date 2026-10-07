import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SquareTerminal } from "lucide-react";
import type { Os, SshAgentStatus } from "../../lib/types";
import { formatAppError } from "../../lib/errors";
import { t, useLocale } from "../../i18n";

/** Settings > SSH agent: the toggle, why it may not be listening, and the
 * line or two that point ssh and Git at it. */
export function SshAgentSettings({ os, busy }: { os: Os; busy: boolean }) {
  useLocale();
  const [status, setStatus] = useState<SshAgentStatus | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    void invoke<SshAgentStatus>("ssh_agent_status")
      .then((s) => {
        if (!cancelled) setStatus(s);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  const toggle = async (on: boolean) => {
    setError(null);
    setWorking(true);
    try {
      setStatus(await invoke<SshAgentStatus>("ssh_agent_set", { enabled: on }));
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setWorking(false);
    }
  };

  const windows = os === "windows";
  // macOS puts the socket under "Application Support": a path with a space
  // is quoted wherever it is pasted.
  const address =
    status?.address && /\s/.test(status.address) ? `"${status.address}"` : status?.address;
  const profile = os === "macos" ? "~/.zshrc" : t("set.ssh_shell_profile");
  return (
    <div className="panel-section">
      <h3>
        <SquareTerminal size={16} />
        {t("settings.ssh")}
      </h3>
      <p>{t("set.ssh_intro")}</p>
      {status && !status.supported ? (
        <p className="hint">{t("set.not_available_yet")}</p>
      ) : (
        <>
          <label className="s3-checkbox">
            <input
              type="checkbox"
              checked={status?.enabled ?? false}
              disabled={busy || working || !status}
              onChange={(e) => void toggle(e.target.checked)}
            />
            <span>
              {t("set.ssh_turn_on")}
              <span className="hint">{t("set.ssh_turn_on_hint")}</span>
            </span>
          </label>
          {status?.enabled && status.problem && (
            <p className="hint is-error">{status.problem}</p>
          )}
          {error && <p className="hint is-error">{error}</p>}
          {status?.enabled && status.running && status.address && (
            <div className="ssh-agent-setup">
              {windows ? (
                <>
                  <p className="hint">{t("set.ssh_windows")}</p>
                  <pre className="code-line">
                    git config --global core.sshCommand C:/Windows/System32/OpenSSH/ssh.exe
                  </pre>
                </>
              ) : (
                <>
                  <p className="hint">{t("set.ssh_profile", { profile })}</p>
                  <pre className="code-line">export SSH_AUTH_SOCK={address}</pre>
                  <p className="hint">{t("set.ssh_config")}</p>
                  <pre className="code-line">{`Host *\n  IdentityAgent ${address}`}</pre>
                </>
              )}
              <p className="hint">{t("set.ssh_openssh")}</p>
            </div>
          )}
        </>
      )}
    </div>
  );
}
