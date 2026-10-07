import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SquareTerminal } from "lucide-react";
import type { Os, SshAgentStatus } from "../../lib/types";
import { formatAppError } from "../../lib/errors";

/** Settings > SSH agent: the toggle, why it may not be listening, and the
 * line or two that point ssh and Git at it. */
export function SshAgentSettings({ os, busy }: { os: Os; busy: boolean }) {
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
  const profile = os === "macos" ? "~/.zshrc" : "your shell's profile";
  return (
    <div className="panel-section">
      <h3>
        <SquareTerminal size={16} />
        SSH agent
      </h3>
      <p>
        Lets ssh, Git and your editor use the SSH keys in the silo that is open. The keys never
        leave SilentSilo, and you confirm each use in this window.
      </p>
      {status && !status.supported ? (
        <p className="hint">Not available on this system yet.</p>
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
              Turn on the SSH agent
              <span className="hint">
                Only keys marked "Use with the SSH agent" are offered. A locked silo offers
                nothing.
              </span>
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
                  <p className="hint">
                    Windows' own ssh finds the agent by itself. Git for Windows uses its own ssh;
                    point it at Windows' with:
                  </p>
                  <pre className="code-line">
                    git config --global core.sshCommand C:/Windows/System32/OpenSSH/ssh.exe
                  </pre>
                </>
              ) : (
                <>
                  <p className="hint">Point ssh at the agent in {profile}:</p>
                  <pre className="code-line">export SSH_AUTH_SOCK={address}</pre>
                  <p className="hint">or for ssh alone, in ~/.ssh/config:</p>
                  <pre className="code-line">{`Host *\n  IdentityAgent ${address}`}</pre>
                </>
              )}
              <p className="hint">
                OpenSSH 8.9 or later tells the agent which server it connects to; with an older
                one, every use is asked.
              </p>
            </div>
          )}
        </>
      )}
    </div>
  );
}
