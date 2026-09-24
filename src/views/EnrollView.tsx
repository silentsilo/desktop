import { useState } from "react";
import { osOf, platformStrings } from "../lib/platformStrings";
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
  const platform = platformStrings(osOf(bootstrap));
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
          A security key (YubiKey, Nitrokey, SoloKeys) works on any computer. {platform.builtIn}{" "}
          is quicker but works only on this one. You can add the other later in Settings.
        </p>
        {bootstrap.fido_available ? (
          <>
            <p className="hint">
              {platform.osName} will show its own prompt.
              {platform.offersPhone &&
                " It may also offer a phone through a QR code. Recent Android phones work; others are refused with a reason."}
            </p>
            {!bootstrap.platform_authenticator && (
              <p className="hint">{platform.builtInSetupHint}</p>
            )}
          </>
        ) : (
          <p className="error">{platform.fidoUnavailable}</p>
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
            If you lose what you choose here, only a recovery code gets you
            back in. Without one, the files are lost for good. We keep no copy
            of your key and cannot open the silo for you.
          </p>
          <p>
            Make a recovery code in Settings as soon as you are in, and keep it
            where you keep your passport.
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
                The key you enrol next belongs to the organisation. The person using this computer
                cannot remove it, or change the recovery code without it. Enrol a second
                organisation key later in Settings: if the only one is lost, nobody can administer
                the silo. This cannot be turned on later.
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
              Use {platform.builtIn}
            </button>
          )}
        </div>
        <div className="auth-alternatives">
          <button type="button" className="secondary" disabled={busy} onClick={onBack}>
            <ArrowLeft size={15} />
            Switch silo
          </button>
        </div>
        <p className="hint danger-hint">
          Not setting this one up?{" "}
          <button type="button" className="link" disabled={busy} onClick={onDiscard}>
            Remove it from the list
          </button>
          . The folder stays on disk.
        </p>
      </section>
    </AuthShell>
  );
}
