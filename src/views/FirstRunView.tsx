import { useState } from "react";
import { AlertTriangle, CloudUpload, Copy, LifeBuoy } from "lucide-react";
import { AuthShell } from "../layout/AuthShell";
import { EmergencyKitPanel } from "./EmergencyKitPanel";

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
  const [step, setStep] = useState<"code" | "backup">("code");
  const [code, setCode] = useState<string | null>(null);
  const [skippedCode, setSkippedCode] = useState(false);

  if (step === "code") {
    return (
      <AuthShell subtitle={`Step 1 of 2: a way back into ${siloName}`}>
        <section className="card auth-card">
          <h2>
            <LifeBuoy size={18} /> Recovery code
          </h2>
          <p className="hint">Step 1 of 2</p>
          {code ? (
            <>
              <p className="hint is-error">
                <AlertTriangle size={14} />
                Shown once. Write it down or print the kit before you go on.
              </p>
              <code className="recovery-code">{code}</code>
              <div className="actions">
                <button type="button" className="secondary" onClick={() => onCopyCode(code)}>
                  <Copy size={15} />
                  Copy
                </button>
              </div>
              <EmergencyKitPanel busy={busy} siloId={siloId} siloName={siloName} freshCode={code} />
              <div className="auth-primary">
                <button type="button" onClick={() => setStep("backup")}>
                  I&apos;ve written it down
                </button>
              </div>
            </>
          ) : (
            <>
              <p>
                One long code that opens this silo when every key is lost, on any computer. Keep
                it on paper, somewhere safe.
              </p>
              <div className="auth-primary">
                <button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void onCreateCode().then((made) => {
                      if (made) setCode(made);
                    })
                  }
                >
                  {busy ? <span className="spinner" aria-hidden /> : <LifeBuoy size={17} />}
                  Create a recovery code
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
                  Not now
                </button>
              </div>
              <p className="hint">
                Without one, losing every key means losing the silo. You can make it later under
                Settings, Recovery code.
              </p>
            </>
          )}
        </section>
      </AuthShell>
    );
  }

  return (
    <AuthShell subtitle={`Step 2 of 2: a copy of ${siloName} somewhere else`}>
      <section className="card auth-card">
        <h2>
          <CloudUpload size={18} /> Backup storage
        </h2>
        <p className="hint">Step 2 of 2</p>
        <p>
          This silo is only on this computer. Backup storage keeps an encrypted copy on a drive,
          NAS, cloud bucket or server you control, and lets another computer set it up too.
        </p>
        {skippedCode && (
          <p className="hint">You skipped the recovery code. Settings, Overview reminds you until you make one.</p>
        )}
        <div className="auth-primary">
          <button type="button" onClick={() => onFinish("backup")}>
            <CloudUpload size={17} />
            Set up backup storage
          </button>
        </div>
        <div className="auth-alt">
          <button type="button" className="link" onClick={() => onFinish(null)}>
            Set up later
          </button>
        </div>
        <p className="hint">
          Until then, if this computer fails, the silo is lost with it. You can set it up any time
          under Settings, Backup.
        </p>
      </section>
    </AuthShell>
  );
}
