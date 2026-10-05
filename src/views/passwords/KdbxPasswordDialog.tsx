import { useState } from "react";
import { useModal } from "../../hooks/useModal";
import { open as openFileDialog } from "../../lib/dialog";

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
  const cardRef = useModal(onCancel);
  const [password, setPassword] = useState("");
  const [repeat, setRepeat] = useState("");
  const [keyFile, setKeyFile] = useState<string | null>(null);

  const exporting = mode === "export";
  const problem = exporting
    ? password.length < MIN_EXPORT_LENGTH
      ? `At least ${MIN_EXPORT_LENGTH} characters.`
      : password !== repeat
        ? "The two do not match."
        : null
    : !password && !keyFile
      ? "The password, the key file, or both."
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
    <div className="modal-overlay" onClick={onCancel}>
      <div
        ref={cardRef}
        className="modal-card"
        role="dialog"
        aria-label={exporting ? "Password for the KeePass file" : "Open the KeePass database"}
        onClick={(e) => e.stopPropagation()}
      >
        <h3>{exporting ? "Protect the KeePass file" : "Open the KeePass database"}</h3>
        <p>
          {exporting
            ? "The file holds every entry, with its passwords, fields, files and history. This password is all that protects it once it leaves the silo, so make it long."
            : "The password KeePass asks for when it opens this file."}
        </p>
        <label className="field field-full">
          <span>Password</span>
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
            <span>Again</span>
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
            <span>Key file</span>
            <div className="pw-kdbx-keyfile">
              <span className="hint">{keyFile ?? "None"}</span>
              <button type="button" className="btn" onClick={() => void chooseKeyFile()}>
                {keyFile ? "Change" : "Choose"}
              </button>
              {keyFile && (
                <button type="button" className="link" onClick={() => setKeyFile(null)}>
                  Remove
                </button>
              )}
            </div>
          </div>
        )}
        {(error || (exporting && password && problem)) && (
          <p className="hint is-error">{error ?? problem}</p>
        )}
        <div className="modal-actions">
          <button type="button" className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button
            type="button"
            className="btn btn-primary"
            disabled={busy || problem !== null}
            onClick={submit}
          >
            {busy ? "Working…" : exporting ? "Export" : "Open"}
          </button>
        </div>
      </div>
    </div>
  );
}
