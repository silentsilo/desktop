import { AlertTriangle, ArrowLeft, Fingerprint, KeyRound } from "lucide-react";
import type { Authenticator, Bootstrap } from "../lib/types";
import { AuthShell } from "../layout/AuthShell";

type Props = {
  bootstrap: Bootstrap;
  busy: boolean;
  fidoProgress: string | null;
  onRetry: () => void;
  onEnroll: (authenticator: Authenticator) => void;
  onDiscard: () => void;
  onBack: () => void;
};

export function EnrollView({
  bootstrap,
  busy,
  fidoProgress,
  onRetry,
  onEnroll,
  onDiscard,
  onBack,
}: Props) {
  return (
    <AuthShell
      title="SilentSilo"
      subtitle={`Choose what unlocks ${bootstrap.silo?.name ?? "this silo"}.`}
    >
      <section className="card auth-card">
        <h2>Set up unlocking</h2>
        <p className="hint">
          A FIDO2 key (YubiKey, Nitrokey, SoloKeys and others) travels with you and survives this
          computer. Windows Hello is quicker, but it is sealed to this machine: if the machine
          dies, so does that way in. Both protect the silo equally well, and you can add the
          other later in Settings.
        </p>
        {bootstrap.fido_available ? (
          <>
            <p className="hint">
              Security key ready. Windows will show its own prompt. If it offers to use a phone
              via a QR code, that one will not work: a passkey stored on a phone cannot unlock a
              silo.
            </p>
            {!bootstrap.platform_authenticator && (
              <p className="hint">
                Windows Hello is not set up on this machine, so a security key is the only way in
                here. Add a PIN or a fingerprint in Windows sign-in settings and Hello shows up as
                a second option.
              </p>
            )}
          </>
        ) : (
          <p className="error">
            FIDO2 is not available on this system. Use Windows 10 (1903+) or later with a compatible
            security key.
          </p>
        )}
        {/* Said before the choice, not after it. Whichever way in they pick,
            this is the part that decides whether the silo survives a bad
            day, and it is the one thing about the design that cannot be
            fixed later by us. */}
        <div className="consequence">
          <h3>
            <AlertTriangle size={15} />
            What happens if you lose it
          </h3>
          <p>
            Whatever you choose here is the way in. If you lose it, a
            recovery code is the only thing that gets you back, and if you
            never made one, or you lose that too, the files are gone
            permanently. There is no backdoor, no support override and no
            copy of your key on our side.
          </p>
          <p>
            That is the direct consequence of the thing that makes this worth
            using: nobody else can open your silo, including us. So make a
            recovery code as soon as you are in, from Settings, and keep it
            where you would keep a passport.
          </p>
        </div>
        {fidoProgress && <p className="fido-live">{fidoProgress}</p>}
        <div className="actions">
          {!bootstrap.fido_available && (
            <button type="button" className="secondary" disabled={busy} onClick={onRetry}>
              {busy ? "Checking…" : "Retry detection"}
            </button>
          )}
          <button
            type="button"
            disabled={busy || !bootstrap.fido_available}
            onClick={() => onEnroll("security-key")}
          >
            <KeyRound size={15} />
            {busy ? "Waiting…" : "Use a security key"}
          </button>
          {bootstrap.platform_authenticator && (
            <button
              type="button"
              className="secondary"
              disabled={busy}
              onClick={() => onEnroll("this-device")}
            >
              <Fingerprint size={15} />
              Use Windows Hello
            </button>
          )}
        </div>
        <div className="auth-alternatives">
          <button type="button" className="secondary" disabled={busy} onClick={onBack}>
            <ArrowLeft size={15} />
            Back to silos
          </button>
        </div>
        <p className="hint danger-hint">
          No key to hand right now? The silo is still empty, so nothing is lost if you{" "}
          <button type="button" className="link" disabled={busy} onClick={onDiscard}>
            discard it
          </button>
          .
        </p>
      </section>
    </AuthShell>
  );
}
