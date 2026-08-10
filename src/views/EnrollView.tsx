import { useState } from "react";
import { AlertTriangle, ArrowLeft, Building2, Fingerprint, KeyRound } from "lucide-react";
import type { Authenticator, Bootstrap } from "../lib/types";
import { AuthShell } from "../layout/AuthShell";

type Props = {
  bootstrap: Bootstrap;
  busy: boolean;
  fidoProgress: string | null;
  onRetry: () => void;
  onEnroll: (authenticator: Authenticator, organisation: boolean) => void;
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
  /// Off unless someone deliberately says otherwise, and only offered here.
  /// A key its holder cannot remove has to be part of what the silo was set
  /// up as: added later to a silo somebody is already using, the same feature
  /// would be a way to take their vault away from them.
  const [organisation, setOrganisation] = useState(false);

  return (
    <AuthShell
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
              Security key ready. Windows will show its own prompt. It may also offer a phone
              via a QR code: that works on recent Android phones, and a phone whose passkey
              cannot derive the silo key is refused with an explanation.
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
        {/* The one moment this can be chosen, so it is asked here rather than
            offered as a setting later. Unticked is the ordinary case and the
            default: almost every silo belongs to the person setting it up. */}
        <label className="confirm-option org-option">
          <input
            type="checkbox"
            checked={organisation}
            disabled={busy}
            onChange={(e) => setOrganisation(e.target.checked)}
          />
          <span>
            <Building2 size={14} aria-hidden /> This silo is administered by an organisation
            {/* The details show once the box is ticked. Almost every silo is
                personal, and three sentences about escrow on every first run
                made the screen longer than a short window, for a choice most
                people rightly skip. The one-line label is enough to find; the
                consequences appear before the enrolment they apply to. */}
            {organisation && (
              <span className="hint">
                For a company setting a silo up for someone. The key you enrol next stays the
                organisation&apos;s way in: whoever uses this computer cannot remove it, and cannot
                change the recovery code without it. Enrol a second organisation key afterwards,
                from Settings, because losing the only one leaves nobody able to administer the
                silo. This cannot be turned on later, and it is visible on every device.
              </span>
            )}
          </span>
        </label>
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
            onClick={() => onEnroll("security-key", organisation)}
          >
            <KeyRound size={15} />
            {busy ? "Waiting…" : "Use a security key"}
          </button>
          {/* Hidden rather than disabled once the organisation box is
              ticked: Hello is sealed to this machine, and an organisation
              key has to open the silo from anywhere. The backend refuses
              the combination too. */}
          {bootstrap.platform_authenticator && !organisation && (
            <button
              type="button"
              className="secondary"
              disabled={busy}
              onClick={() => onEnroll("this-device", false)}
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
