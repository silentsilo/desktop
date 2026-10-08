import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { PasswordEntry } from "../../lib/types";
import { formatAppError } from "../../lib/errors";
import { t, useLocale } from "../../i18n";

/**
 * "Use with the SSH agent" on an SSH-key entry. Turning it on checks that
 * the agent can read the key; a key with a passphrase is opened here once
 * and kept without it, the version with the passphrase going into the
 * entry's history when it is saved.
 */
export function SshAgentOption({
  draft,
  onChange,
}: {
  draft: PasswordEntry;
  onChange: (changes: Partial<PasswordEntry>) => void;
}) {
  useLocale();
  const [asking, setAsking] = useState(false);
  const [passphrase, setPassphrase] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const key = draft.ssh_private_key ?? "";

  const turn = async (on: boolean) => {
    setError(null);
    setAsking(false);
    if (!on) {
      onChange({ ssh_agent: undefined });
      return;
    }
    setBusy(true);
    try {
      const verdict = await invoke<string>("ssh_key_check", { key });
      if (verdict === "ok") onChange({ ssh_agent: true });
      else if (verdict === "encrypted") setAsking(true);
      else if (verdict === "unsupported")
        setError(t("pw.ssh_unsupported"));
      else
        setError(t("pw.ssh_unreadable"));
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setBusy(false);
    }
  };

  const open = async () => {
    setError(null);
    setBusy(true);
    try {
      const plain = await invoke<string>("ssh_key_remove_passphrase", {
        key,
        passphrase,
      });
      onChange({ ssh_private_key: plain, ssh_agent: true });
      setAsking(false);
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setPassphrase("");
      setBusy(false);
    }
  };

  return (
    <div className="field field-full">
      <label className="confirm-option">
        <input
          type="checkbox"
          checked={draft.ssh_agent === true}
          disabled={busy || key.trim() === ""}
          onChange={(e) => void turn(e.target.checked)}
        />
        <span>
          {t("pw.ssh_use_agent")}
          <span className="hint">{t("pw.ssh_use_agent_hint")}</span>
        </span>
      </label>
      {asking && (
        <div className="ssh-passphrase">
          <p className="hint">{t("pw.ssh_passphrase_hint")}</p>
          <input
            type="password"
            autoComplete="off"
            value={passphrase}
            disabled={busy}
            aria-label={t("pw.ssh_passphrase")}
            onChange={(e) => setPassphrase(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && passphrase) void open();
            }}
          />
          <button
            className="btn-primary"
            type="button"
            disabled={busy || !passphrase}
            onClick={() => void open()}
          >
            {t("pw.ssh_remove_passphrase")}
          </button>
        </div>
      )}
      {error && <p className="hint is-error">{error}</p>}
    </div>
  );
}
