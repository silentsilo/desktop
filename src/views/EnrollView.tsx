import { useState } from "react";
import { osOf, platformStrings } from "../lib/platformStrings";
import { AlertTriangle, ArrowLeft, Building2, Fingerprint, KeyRound } from "lucide-react";
import type { Authenticator, Bootstrap } from "../lib/types";
import { AuthShell } from "../layout/AuthShell";
import { t, tx, useLocale } from "../i18n";

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
  useLocale();
  const platform = platformStrings(osOf(bootstrap));
  /// Off unless someone deliberately says otherwise, and only offered here.
  /// A key its holder cannot remove has to be part of what the silo was set
  /// up as: added later to a silo somebody is already using, the same feature
  /// would be a way to take their vault away from them.
  const [organisation, setOrganisation] = useState(false);

  return (
    <AuthShell
      subtitle={
        bootstrap.silo?.name
          ? t("start.enroll_subtitle", { name: bootstrap.silo.name })
          : t("start.enroll_subtitle_this")
      }
    >
      <section className="card auth-card">
        <h2>{t("start.enroll_title")}</h2>
        <p className="hint">
          {t("start.enroll_security_key")}
          {platform.hasBuiltIn && ` ${t("start.enroll_built_in", { builtIn: platform.builtIn })}`}
        </p>
        {bootstrap.fido_available ? (
          <>
            <p className="hint">
              {t("start.enroll_os_prompt", { os: platform.osName })}
              {platform.offersPhone && ` ${t("start.enroll_phone")}`}
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
            {t("start.enroll_lose_title")}
          </h3>
          <p>{t("start.enroll_lose_body")}</p>
          <p>{t("start.enroll_lose_code")}</p>
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
            <Building2 size={14} aria-hidden /> {t("start.enroll_org")}
            {/* The details show once the box is ticked. Almost every silo is
                personal, and three sentences about escrow on every first run
                made the screen longer than a short window, for a choice most
                people rightly skip. The one-line label is enough to find; the
                consequences appear before the enrolment they apply to. */}
            {organisation && (
              <span className="hint">{t("start.enroll_org_detail")}</span>
            )}
          </span>
        </label>
        {fidoProgress && <p className="fido-live">{fidoProgress}</p>}
        <div className="actions">
          {!bootstrap.fido_available && (
            <button type="button" className="btn-secondary" disabled={busy} onClick={onRetry}>
              {busy ? t("start.checking") : t("start.enroll_retry")}
            </button>
          )}
          <button
            className="btn-primary"
            type="button"
            disabled={busy || !bootstrap.fido_available}
            onClick={() => onEnroll("security-key", organisation)}
          >
            <KeyRound size={15} />
            {busy ? t("start.waiting") : t("start.enroll_use_key")}
          </button>
          {/* Hidden rather than disabled once the organisation box is
              ticked: Hello is sealed to this machine, and an organisation
              key has to open the silo from anywhere. The backend refuses
              the combination too. */}
          {bootstrap.platform_authenticator && !organisation && (
            <button
              type="button"
              className="btn-secondary"
              disabled={busy}
              onClick={() => onEnroll("this-device", false)}
            >
              <Fingerprint size={15} />
              {t("start.enroll_use_built_in", { builtIn: platform.builtIn })}
            </button>
          )}
        </div>
        <div className="auth-alternatives">
          <button type="button" className="btn-secondary" disabled={busy} onClick={onBack}>
            <ArrowLeft size={15} />
            {t("start.switch_silo")}
          </button>
        </div>
        <p className="hint danger-hint">
          {tx("start.enroll_discard", {
            link: (
              <button type="button" className="link" disabled={busy} onClick={onDiscard}>
                {t("start.enroll_discard_link")}
              </button>
            ),
          })}
        </p>
      </section>
    </AuthShell>
  );
}
