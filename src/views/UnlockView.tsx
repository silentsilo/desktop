import { useState } from "react";
import { osOf, platformStrings } from "../lib/platformStrings";
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
  /** The key found this computer's copy damaged and the user agreed to
   * rebuild it, which needs the recovery code. */
  rebuilding?: boolean;
  onCancelRebuild?: () => void;
};

export function UnlockView({
  bootstrap,
  busy,
  fidoProgress,
  onRetry,
  onUnlock,
  onUnlockWithRecovery,
  onSwitchSilo,
  rebuilding = false,
  onCancelRebuild,
}: Props) {
  const platform = platformStrings(osOf(bootstrap));
  const [usingCode, setUsingCode] = useState(false);
  const [code, setCode] = useState("");

  if (usingCode || rebuilding) {
    return (
      <AuthShell
        subtitle={
          rebuilding
            ? "Enter the code you wrote down to rebuild this silo from backup storage."
            : "Unlock with the code you wrote down."
        }
      >
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
            The code you saved when you set this up. Paste the whole code into any box and the
            rest fill in.
          </p>
          <div className="field">
            <span>Code</span>
            <RecoveryCodeInput value={code} onChange={setCode} disabled={busy} autoFocus />
          </div>
          <div className="actions">
            <button type="submit" disabled={busy || !isComplete(code)}>
              <LifeBuoy size={15} />
              {busy ? "Checking…" : rebuilding ? "Rebuild and unlock" : "Unlock"}
            </button>
            <button
              type="button"
              className="secondary"
              disabled={busy}
              onClick={() => {
                setUsingCode(false);
                setCode("");
                onCancelRebuild?.();
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
    ? `Confirm with ${platform.builtIn} to unlock.`
    : both
      ? `Touch an enrolled security key, or confirm with ${platform.builtIn}, to unlock.`
      : "Insert an enrolled security key and touch it to unlock.";
  // Only that the prompt is available: no key has been looked at yet.
  const readyHint = `${platform.osName} will show its own prompt.`;

  return (
    <AuthShell subtitle={subtitle}>
      <section className="card auth-card">
        <h2>{bootstrap.silo?.name ?? "Unlock silo"}</h2>
        {bootstrap.fido_available ? (
          <p className="hint">{readyHint}</p>
        ) : (
          <p className="error">{platform.fidoUnavailable}</p>
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
              ? `${platform.builtIn} not working? Use your recovery code`
              : "Lost your key? Use your recovery code"}
          </button>
          <button type="button" className="link" disabled={busy} onClick={onSwitchSilo}>
            <HardDrive size={14} />
            Switch silo
          </button>
        </div>
      </section>
    </AuthShell>
  );
}
