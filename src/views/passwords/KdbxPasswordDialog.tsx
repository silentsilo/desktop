import { useState } from "react";
import { useModal } from "../../hooks/useModal";
import { open as openFileDialog } from "../../lib/dialog";
import { t, useLocale } from "../../i18n";

type Props = {
  /** Opening a database someone else made, or writing a new one. */
  mode: "open" | "export";
  busy: boolean;
  error: string | null;
  onSubmit: (password: string, keyFile: string | null) => void;
  onCancel: () => void;
};

/** Below this an exported file is too easy to guess offline. */
const MIN_EXPORT_LENGTH = 8;

/**
 * The password of a KeePass database: the one that opens a file being
 * imported, with its key file if it has one, or a new one for an export,
 * typed twice. Nothing here is kept once the dialog closes.
 */
export function KdbxPasswordDialog({ mode, busy, error, onSubmit, onCancel }: Props) {
  useLocale();
  // While it works the files are already being written into the silo:
  // cancelling then would leave them with nothing to point at.
  const cancel = busy ? undefined : onCancel;
  const cardRef = useModal(cancel);
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [keyFile, setKeyFile] = useState<string | null>(null);

  const exporting = mode === "export";
  const problem = exporting
    ? password.length < MIN_EXPORT_LENGTH
      ? t("pw.kdbx_min_length", { count: MIN_EXPORT_LENGTH })
      : password !== repeat
        ? t("pw.kdbx_mismatch")
        : null
    : !password && !keyFile
      ? t("pw.kdbx_need_secret")
      : null;

  const submit = () => {
    if (!problem && !busy) onSubmit(password, keyFile);
  };

  const chooseKeyFile = async () => {
    const picked = await openFileDialog({ multiple: false });
    const path = typeof picked === "string" ? picked : picked?.[0];
    if (path) setKeyFile(path);
  };

  return (
    <div className="modal-overlay" onClick={cancel}>
      <div
        ref={cardRef}
        className="modal-card"
        role="dialog"
        aria-modal="true"
        aria-label={exporting ? t("pw.kdbx_export_label") : t("pw.kdbx_open_title")}
        onClick={(e) => e.stopPropagation()}
      >
        <h3>{exporting ? t("pw.kdbx_export_title") : t("pw.kdbx_open_title")}</h3>
        <p>
          {exporting ? t("pw.kdbx_export_body") : t("pw.kdbx_open_body")}
        </p>
        <label className="field field-full">
          <span>{t("pw.field_password")}</span>
          <input
            autoFocus
            type="password"
            autoComplete={exporting ? "new-password" : "current-password"}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !exporting) submit();
            }}
          />
        </label>
        {exporting ? (
          <label className="field field-full">
            <span>{t("pw.kdbx_again")}</span>
            <input
              type="password"
              autoComplete="new-password"
              value={repeat}
              onChange={(e) => setRepeat(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
          </label>
        ) : (
          <div className="field field-full">
            <span>{t("pw.kdbx_key_file")}</span>
            <div className="pw-kdbx-keyfile">
              <span className="hint">{keyFile ?? t("pw.kdbx_no_key_file")}</span>
              <button type="button" className="secondary" onClick={() => void chooseKeyFile()}>
                {keyFile ? t("pw.kdbx_change") : t("pw.kdbx_choose")}
              </button>
              {keyFile && (
                <button type="button" className="link" onClick={() => setKeyFile(null)}>
                  {t("pw.remove")}
                </button>
              )}
            </div>
          </div>
        )}
        {(error || (exporting && password && problem)) && (
          <p className="hint is-error">{error ?? problem}</p>
        )}
        <div className="modal-actions">
          <button type="button" className="secondary" onClick={onCancel} disabled={busy}>
            {t("common.cancel")}
          </button>
          <button
            type="button"
            className="primary"
            disabled={busy || problem !== null}
            onClick={submit}
          >
            {busy ? t("pw.kdbx_working") : exporting ? t("pw.export") : t("pw.kdbx_open")}
          </button>
        </div>
      </div>
    </div>
  );
}
