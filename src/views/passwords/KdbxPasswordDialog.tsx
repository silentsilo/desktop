import { useState } from "react";
import { useModal } from "../../hooks/useModal";
import { open as openFileDialog } from "../../lib/dialog";
import { t, useLocale } from "../../i18n";
import { DEFAULT_GEN_OPTIONS, generatePassphrase } from "./util";

type Props = {
  /** Opening a database someone else made, or writing a new one. */
  mode: "open" | "export";
  /** A KeePass database, or a SilentSilo export, which has no key file. */
  format?: "kdbx" | "silentsilo";
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
export function KdbxPasswordDialog({
  mode,
  format = "kdbx",
  busy,
  error,
  onSubmit,
  onCancel,
}: Props) {
  useLocale();
  // While it works the files are already being written into the silo:
  // cancelling then would leave them with nothing to point at.
  const cancel = busy ? undefined : onCancel;
  const cardRef = useModal(cancel);
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [keyFile, setKeyFile] = useState<string | null>(null);
  /// A suggested password is shown, so it can be written down or read out.
  const [shown, setShown] = useState(false);
  const own = format === "silentsilo";

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
        aria-label={
          own
            ? exporting
              ? t("pw.sse_export_title")
              : t("pw.sse_open_title")
            : exporting
              ? t("pw.kdbx_export_label")
              : t("pw.kdbx_open_title")
        }
        onClick={(e) => e.stopPropagation()}
      >
        <h3>
          {own
            ? exporting
              ? t("pw.sse_export_title")
              : t("pw.sse_open_title")
            : exporting
              ? t("pw.kdbx_export_title")
              : t("pw.kdbx_open_title")}
        </h3>
        <p>
          {own
            ? exporting
              ? t("pw.sse_export_body")
              : t("pw.sse_open_body")
            : exporting
              ? t("pw.kdbx_export_body")
              : t("pw.kdbx_open_body")}
        </p>
        <label className="field field-full">
          <span>{t("pw.field_password")}</span>
          <input
            autoFocus
            type={shown ? "text" : "password"}
            autoComplete={exporting ? "new-password" : "current-password"}
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !exporting) submit();
            }}
          />
        </label>
        {exporting && (
          <button
            type="button"
            className="link"
            onClick={() => {
              const suggested = generatePassphrase({ ...DEFAULT_GEN_OPTIONS, kind: "words", words: 6 });
              setPassword(suggested);
              setRepeat(suggested);
              setShown(true);
            }}
          >
            {t("pw.export_suggest_password")}
          </button>
        )}
        {exporting ? (
          <label className="field field-full">
            <span>{t("pw.kdbx_again")}</span>
            <input
              type={shown ? "text" : "password"}
              autoComplete="new-password"
              value={repeat}
              onChange={(e) => setRepeat(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submit();
              }}
            />
          </label>
        ) : own ? null : (
          <div className="field field-full">
            <span>{t("pw.kdbx_key_file")}</span>
            <div className="pw-kdbx-keyfile">
              <span className="hint">{keyFile ?? t("pw.kdbx_no_key_file")}</span>
              <button type="button" className="btn-secondary" onClick={() => void chooseKeyFile()}>
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
          <button type="button" className="btn-secondary" onClick={onCancel} disabled={busy}>
            {t("common.cancel")}
          </button>
          <button
            type="button"
            className="btn-primary"
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
