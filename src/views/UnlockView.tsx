import { useState } from "react";
import { ArrowLeft, HardDrive, KeyRound, LifeBuoy } from "lucide-react";
import type { Bootstrap } from "../lib/types";
import { AuthShell } from "../layout/AuthShell";
import { RecoveryCodeInput } from "../components/RecoveryCodeInput";
import { isComplete } from "../lib/recoveryCode";

type Props = {
  bootstrap: Bootstrap;
  busy: boolean;
  fidoProgress: string | null;
  onRetry: () => void;
  onUnlock: () => void;
  onUnlockWithRecovery: (code: string) => void;
  onSwitchSilo: () => void;
};

export function UnlockView({
  bootstrap,
  busy,
  fidoProgress,
  onRetry,
  onUnlock,
  onUnlockWithRecovery,
  onSwitchSilo,
}: Props) {
  const [usingCode, setUsingCode] = useState(false);
  const [code, setCode] = useState("");

  if (usingCode) {
    return (
      <AuthShell subtitle="Unlock with the code you wrote down.">
        {/* A real form, so Enter in the field submits rather than doing
            nothing and sending the user back to the mouse. */}
        <form
          className="card auth-card"
          onSubmit={(e) => {
            e.preventDefault();
            if (!busy && isComplete(code)) onUnlockWithRecovery(code);
          }}
        >
          <h2>Recovery code</h2>
          <p className="hint">
            The long code you saved when you set this up, one group per box. Paste the whole
            code into any of them and the rest fill themselves.
          </p>
          <div className="field">
            <span>Code</span>
            <RecoveryCodeInput value={code} onChange={setCode} disabled={busy} autoFocus />
          </div>
          <div className="actions">
            <button type="submit" disabled={busy || !isComplete(code)}>
              <LifeBuoy size={15} />
              {busy ? "Checking…" : "Unlock"}
            </button>
            <button
              type="button"
              className="secondary"
              disabled={busy}
              onClick={() => {
                setUsingCode(false);
                setCode("");
              }}
            >
              <ArrowLeft size={15} />
              Back
            </button>
          </div>
        </form>
      </AuthShell>
    );
  }

  // Worded around what is actually enrolled. Telling a silo whose only way
  // in is Windows Hello to insert a key sends its owner looking for hardware
  // they do not have.
  const helloOnly = bootstrap.platform_enrolled && !bootstrap.portable_enrolled;
  const both = bootstrap.platform_enrolled && bootstrap.portable_enrolled;
  const subtitle = helloOnly
    ? "Confirm with Windows Hello to unlock."
    : both
      ? "Touch an enrolled security key, or confirm with Windows Hello, to unlock."
      : "Insert an enrolled security key and touch it to unlock.";
  const readyHint = helloOnly
    ? "Windows Hello is ready. Windows will show its own prompt."
    : "Security key ready. Windows will show a native prompt.";

  return (
    <AuthShell subtitle={subtitle}>
      <section className="card auth-card">
        <h2>{bootstrap.silo?.name ?? "Unlock silo"}</h2>
        {bootstrap.fido_available ? (
          <p className="hint">{readyHint}</p>
        ) : (
          <p className="error">
            FIDO2 is not available on this system. Use Windows 10 (1903+) or later with a compatible
            security key.
          </p>
        )}
        {fidoProgress && <p className="fido-live">{fidoProgress}</p>}

        {/* The one thing this screen exists for, given the room to say so. */}
        <div className="auth-primary">
          {!bootstrap.fido_available && (
            <button type="button" className="secondary" disabled={busy} onClick={onRetry}>
              {busy ? "Checking…" : "Retry detection"}
            </button>
          )}
          <button type="button" disabled={busy || !bootstrap.fido_available} onClick={onUnlock}>
            {busy ? <span className="spinner" aria-hidden /> : <KeyRound size={17} />}
            <span>{busy ? "Waiting…" : "Unlock"}</span>
          </button>
        </div>

        {/* Ruled off below it: both are ways out of this screen rather than
            ways through it, and stacked directly under the button they read
            as a third and fourth thing to try first. */}
        <div className="auth-alt">
          <button type="button" className="link" disabled={busy} onClick={() => setUsingCode(true)}>
            <LifeBuoy size={14} />
            {helloOnly
              ? "Hello not working? Use your recovery code"
              : "Lost your key? Use your recovery code"}
          </button>
          <button type="button" className="link" disabled={busy} onClick={onSwitchSilo}>
            <HardDrive size={14} />
            Open a different silo
          </button>
        </div>
      </section>
    </AuthShell>
  );
}
