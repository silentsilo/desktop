import { useState } from "react";
import { AlertTriangle, CloudUpload, Copy, LifeBuoy } from "lucide-react";
import { AuthShell } from "../layout/AuthShell";
import { EmergencyKitPanel } from "./EmergencyKitPanel";
import { t, useLocale } from "../i18n";

type Props = {
  siloId: string;
  siloName: string;
  busy: boolean;
  /** Makes a recovery code and hands it back, or null when it failed. */
  onCreateCode: () => Promise<string | null>;
  onCopyCode: (code: string) => void;
  /** Leaves the guide, to the backup page or to the files. */
  onFinish: (next: "backup" | null) => void;
};

/**
 * The two steps a new silo needs before it is safe, right after its first
 * key: a recovery code, then backup storage. Either can wait, and the
 * screen says what waiting means.
 */
export function FirstRunView({ siloId, siloName, busy, onCreateCode, onCopyCode, onFinish }: Props) {
  useLocale();
  const [step, setStep] = useState<"code" | "backup">("code");
  const [code, setCode] = useState<string | null>(null);
  const [skippedCode, setSkippedCode] = useState(false);

  if (step === "code") {
    return (
      <AuthShell subtitle={t("first.subtitle_code", { name: siloName })}>
        <section className="card auth-card">
          <h2>
            <LifeBuoy size={18} /> {t("first.code_title")}
          </h2>
          <p className="hint">{t("first.step1")}</p>
          {code ? (
            <>
              <p className="hint is-error">
                <AlertTriangle size={14} />
                {t("first.shown_once")}
              </p>
              <code className="recovery-code">{code}</code>
              <div className="actions">
                <button type="button" className="btn-secondary" onClick={() => onCopyCode(code)}>
                  <Copy size={15} />
                  {t("first.copy")}
                </button>
              </div>
              <EmergencyKitPanel busy={busy} siloId={siloId} siloName={siloName} freshCode={code} />
              <div className="auth-primary">
                <button className="btn-primary" type="button" onClick={() => setStep("backup")}>
                  {t("first.written")}
                </button>
              </div>
            </>
          ) : (
            <>
              <p>{t("first.code_intro")}</p>
              <div className="auth-primary">
                <button
                  className="btn-primary"
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void onCreateCode().then((made) => {
                      if (made) setCode(made);
                    })
                  }
                >
                  {busy ? <span className="spinner" aria-hidden /> : <LifeBuoy size={17} />}
                  {t("first.create_code")}
                </button>
              </div>
              <div className="auth-alt">
                <button
                  type="button"
                  className="link"
                  disabled={busy}
                  onClick={() => {
                    setSkippedCode(true);
                    setStep("backup");
                  }}
                >
                  {t("first.not_now")}
                </button>
              </div>
              <p className="hint">{t("first.without_code")}</p>
            </>
          )}
        </section>
      </AuthShell>
    );
  }

  return (
    <AuthShell subtitle={t("first.subtitle_backup", { name: siloName })}>
      <section className="card auth-card">
        <h2>
          <CloudUpload size={18} /> {t("first.backup_title")}
        </h2>
        <p className="hint">{t("first.step2")}</p>
        <p>{t("first.backup_intro")}</p>
        {skippedCode && (
          <p className="hint">{t("first.skipped_code")}</p>
        )}
        <div className="auth-primary">
          <button className="btn-primary" type="button" onClick={() => onFinish("backup")}>
            <CloudUpload size={17} />
            {t("first.setup_backup")}
          </button>
        </div>
        <div className="auth-alt">
          <button type="button" className="link" onClick={() => onFinish(null)}>
            {t("first.later")}
          </button>
        </div>
        <p className="hint">{t("first.until_then")}</p>
      </section>
    </AuthShell>
  );
}
