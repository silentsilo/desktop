import { useState } from "react";
import { osOf, platformStrings } from "../lib/platformStrings";
import {
  AlertTriangle,
  ArrowLeft,
  Building2,
  Check,
  Fingerprint,
  KeyRound,
  Minus,
} from "lucide-react";
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
      <section className="card auth-card is-choice">
        <h2>{t("start.enroll_title")}</h2>
        <p className="hint">{t("start.enroll_lead")}</p>
        {/* The choice itself, as two cards side by side: what each is good
            for and what it costs, so the decision is made on the screen
            rather than in the paragraph above it. */}
        <div className="enroll-choices">
          <button
            type="button"
            className="enroll-choice"
            disabled={busy || !bootstrap.fido_available}
            onClick={() => onEnroll("security-key", organisation)}
          >
            <span className="enroll-choice-icon" aria-hidden>
              {busy ? <span className="spinner" /> : <KeyRound size={20} />}
            </span>
            <strong>{busy ? t("start.waiting") : t("start.enroll_use_key")}</strong>
            <span className="enroll-choice-line is-plus">
              <Check size={13} aria-hidden />
              {t("start.enroll_key_plus")}
            </span>
            <span className="enroll-choice-line">
              <Minus size={13} aria-hidden />
              {t("start.enroll_key_minus")}
            </span>
          </button>
          {/* Hidden rather than disabled once the organisation box is
              ticked: Hello is sealed to this machine, and an organisation
              key has to open the silo from anywhere. The backend refuses
              the combination too. */}
          {bootstrap.platform_authenticator && !organisation && (
            <button
              type="button"
              className="enroll-choice"
              disabled={busy}
              onClick={() => onEnroll("this-device", false)}
            >
              <span className="enroll-choice-icon" aria-hidden>
                <Fingerprint size={20} />
              </span>
              <strong>{t("start.enroll_use_built_in", { builtIn: platform.builtIn })}</strong>
              <span className="enroll-choice-line is-plus">
                <Check size={13} aria-hidden />
                {t("start.enroll_builtin_plus")}
              </span>
              <span className="enroll-choice-line">
                <Minus size={13} aria-hidden />
                {t("start.enroll_builtin_minus")}
              </span>
            </button>
          )}
        </div>
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
        {fidoProgress && <p className="fido-live">{fidoProgress}</p>}
        {!bootstrap.fido_available && (
          <div className="actions">
            <button type="button" className="btn-secondary" disabled={busy} onClick={onRetry}>
              {busy ? t("start.checking") : t("start.enroll_retry")}
            </button>
          </div>
        )}
        {/* Under the choice, and short: whichever way in they pick, this is
            the part that decides whether the silo survives a bad day. */}
        <div className="consequence">
          <h3>
            <AlertTriangle size={15} />
            {t("start.enroll_lose_title")}
          </h3>
          <p>{t("start.enroll_lose_short")}</p>
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
            {/* The details show once the box is ticked: almost every silo
                is personal. */}
            {organisation && (
              <span className="hint">{t("start.enroll_org_detail")}</span>
            )}
          </span>
        </label>
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
