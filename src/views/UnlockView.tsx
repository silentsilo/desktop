import { useState } from "react";
import { osOf, platformStrings } from "../lib/platformStrings";
import { ArrowLeft, HardDrive, KeyRound, LifeBuoy } from "lucide-react";
import type { Bootstrap } from "../lib/types";
import { AuthShell } from "../layout/AuthShell";
import { RecoveryCodeInput } from "../components/RecoveryCodeInput";
import { isComplete } from "../lib/recoveryCode";
import { t, useLocale } from "../i18n";

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
  useLocale();
  const platform = platformStrings(osOf(bootstrap));
  const [usingCode, setUsingCode] = useState(false);
  const [code, setCode] = useState("");

  if (usingCode || rebuilding) {
    return (
      <AuthShell
        subtitle={
          rebuilding
            ? t("unlock.subtitle_rebuild")
            : t("unlock.subtitle_code")
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
          <h2>{t("unlock.recovery_code_title")}</h2>
          <p className="hint">{t("unlock.recovery_code_hint")}</p>
          <div className="field">
            <span>{t("unlock.code_label")}</span>
            <RecoveryCodeInput value={code} onChange={setCode} disabled={busy} autoFocus />
          </div>
          <div className="actions">
            <button className="btn-primary" type="submit" disabled={busy || !isComplete(code)}>
              <LifeBuoy size={15} />
              {busy
                ? t("unlock.checking")
                : rebuilding
                  ? t("unlock.rebuild_and_unlock")
                  : t("unlock.unlock")}
            </button>
            <button
              type="button"
              className="btn-secondary"
              disabled={busy}
              onClick={() => {
                setUsingCode(false);
                setCode("");
                onCancelRebuild?.();
              }}
            >
              <ArrowLeft size={15} />
              {t("unlock.back")}
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
    ? t("unlock.subtitle_builtin", { builtin: platform.builtIn })
    : both
      ? t("unlock.subtitle_both", { builtin: platform.builtIn })
      : t("unlock.subtitle_key");
  // Only that the prompt is available: no key has been looked at yet.
  const readyHint = t("unlock.os_prompt", { os: platform.osName });

  return (
    <AuthShell subtitle={subtitle}>
      <section className="card auth-card">
        <h2>{bootstrap.silo?.name ?? t("unlock.title_fallback")}</h2>
        {bootstrap.fido_available ? (
          <p className="hint">{readyHint}</p>
        ) : (
          <p className="error">{platform.fidoUnavailable}</p>
        )}
        {fidoProgress && <p className="fido-live">{fidoProgress}</p>}

        {/* The one thing this screen exists for, given the room to say so. */}
        <div className="auth-primary">
          {!bootstrap.fido_available && (
            <button type="button" className="btn-secondary" disabled={busy} onClick={onRetry}>
              {busy ? t("unlock.checking") : t("unlock.retry")}
            </button>
          )}
          <button
            className="btn-primary"
            type="button"
            disabled={busy || !bootstrap.fido_available}
            onClick={onUnlock}
          >
            {busy ? <span className="spinner" aria-hidden /> : <KeyRound size={17} />}
            <span>{busy ? t("unlock.waiting") : t("unlock.unlock")}</span>
          </button>
        </div>

        {/* Ruled off below it: both are ways out of this screen rather than
            ways through it, and stacked directly under the button they read
            as a third and fourth thing to try first. */}
        <div className="auth-alt">
          <button type="button" className="link" disabled={busy} onClick={() => setUsingCode(true)}>
            <LifeBuoy size={14} />
            {helloOnly
              ? t("unlock.builtin_not_working", { builtin: platform.builtIn })
              : t("unlock.lost_key")}
          </button>
          <button type="button" className="link" disabled={busy} onClick={onSwitchSilo}>
            <HardDrive size={14} />
            {t("unlock.switch_silo")}
          </button>
        </div>
      </section>
    </AuthShell>
  );
}
