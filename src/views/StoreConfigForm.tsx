import { useState } from "react";
import {
  Cloud,
  FolderOpen,
  HardDrive,
  KeyRound,
  Server,
  ShieldCheck,
  Terminal,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { formatAppError } from "../lib/errors";
import {
  applyPreset,
  DEFAULT_S3_FORM,
  missingFields,
  S3_PRESETS,
  type S3Form,
  type S3Preset,
} from "../lib/s3Presets";
import type { StoreKind } from "../lib/types";
import { S3ConfigForm } from "./S3ConfigForm";
import { PLAIN_HTTP_WARNING, isPlainHttp } from "../lib/plainHttp";

/**
 * Every field any backend needs, held together.
 *
 * All three are kept rather than only the selected one, so switching kinds
 * to compare them and switching back does not silently discard what was
 * typed.
 */
export type SftpForm = {
  host: string;
  /** Text rather than a number so the field can be emptied while typing. */
  port: string;
  username: string;
  path: string;
  method: "password" | "key";
  password: string;
  privateKey: string;
  passphrase: string;
  /** The server key the user has looked at and accepted. */
  fingerprint: string;
};

export type StoreDraft = {
  kind: StoreKind;
  preset: S3Preset;
  s3: S3Form;
  folder: string;
  dav: { url: string; username: string; password: string };
  sftp: SftpForm;
};

export const EMPTY_STORE_DRAFT: StoreDraft = {
  kind: "s3",
  preset: S3_PRESETS[S3_PRESETS.length - 1]!,
  s3: DEFAULT_S3_FORM,
  folder: "",
  dav: { url: "", username: "", password: "" },
  sftp: {
    host: "",
    port: "22",
    username: "",
    path: "",
    method: "password",
    password: "",
    privateKey: "",
    passphrase: "",
    fingerprint: "",
  },
};

/** What the backend expects, for whichever kind is selected. */
export function storeDraftPayload(draft: StoreDraft) {
  switch (draft.kind) {
    case "folder":
      return { kind: "folder" as const, path: draft.folder };
    case "web-dav":
      return {
        kind: "web-dav" as const,
        url: draft.dav.url,
        username: draft.dav.username,
        // Blank is meaningful: the backend reads it as "keep the stored one".
        password: draft.dav.password.trim() ? draft.dav.password : null,
      };
    case "sftp":
      return {
        kind: "sftp" as const,
        host: draft.sftp.host,
        port: Number(draft.sftp.port) || 22,
        username: draft.sftp.username,
        path: draft.sftp.path,
        auth:
          draft.sftp.method === "key"
            ? {
                method: "key" as const,
                privateKey: draft.sftp.privateKey.trim() ? draft.sftp.privateKey : null,
                passphrase: draft.sftp.passphrase.trim() ? draft.sftp.passphrase : null,
              }
            : {
                method: "password" as const,
                password: draft.sftp.password.trim() ? draft.sftp.password : null,
              },
        hostFingerprint: draft.sftp.fingerprint.trim() ? draft.sftp.fingerprint : null,
      };
    default:
      return {
        kind: "s3" as const,
        endpoint: draft.s3.endpoint,
        region: draft.s3.region,
        bucket: draft.s3.bucket,
        prefix: draft.s3.prefix,
        accessKeyId: draft.s3.accessKeyId,
        secretAccessKey: draft.s3.secretAccessKey.trim() ? draft.s3.secretAccessKey : null,
        pathStyle: draft.s3.pathStyle,
      };
  }
}

/**
 * What still has to be filled in, named the way the labels are.
 *
 * `hasStoredSecret` says whether a saved password may stand in for a blank
 * field — true when editing an existing connection, false when describing
 * one for the first time, where there is nothing to fall back on.
 */
export function missingStoreFields(draft: StoreDraft, hasStoredSecret: boolean): string[] {
  switch (draft.kind) {
    case "folder":
      return draft.folder.trim() ? [] : ["folder"];
    case "web-dav":
      return [
        !draft.dav.url.trim() && "address",
        !draft.dav.username.trim() && "username",
        !draft.dav.password.trim() && !hasStoredSecret && "password",
      ].filter((v): v is string => typeof v === "string");
    case "sftp":
      return [
        !draft.sftp.host.trim() && "address",
        !draft.sftp.username.trim() && "username",
        draft.sftp.method === "password"
          ? !draft.sftp.password.trim() && !hasStoredSecret && "password"
          : !draft.sftp.privateKey.trim() && !hasStoredSecret && "private key",
        !draft.sftp.fingerprint.trim() && !hasStoredSecret && "the server's fingerprint",
      ].filter((v): v is string => typeof v === "string");
    default:
      return missingFields(draft.s3, hasStoredSecret);
  }
}

/**
 * Confirming which machine is actually answering.
 *
 * SSH has no certificate authority: the only thing that distinguishes your
 * server from anyone who can answer on that address is its key, and a client
 * that accepts whatever it is offered authenticates nothing. So this is a
 * step of its own rather than a checkbox — the fingerprint is fetched,
 * shown, and only stored once the user says it is theirs.
 *
 * How they check it is the one thing this screen can't do for them: on the
 * server, `ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub` prints the same
 * string.
 */
function HostKeyStep({
  sftp,
  set,
  busy,
}: {
  sftp: SftpForm;
  set: (patch: Partial<SftpForm>) => void;
  busy: boolean;
}) {
  const [offered, setOffered] = useState<string | null>(null);
  const [checking, setChecking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const check = async () => {
    setChecking(true);
    setError(null);
    setOffered(null);
    try {
      setOffered(
        await invoke<string>("sftp_probe_host_key", {
          host: sftp.host.trim(),
          port: Number(sftp.port) || 22,
        }),
      );
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setChecking(false);
    }
  };

  if (sftp.fingerprint && !offered) {
    return (
      <div className="host-key host-key-confirmed">
        <div className="host-key-line">
          <ShieldCheck size={16} />
          <code>{sftp.fingerprint}</code>
        </div>
        <p className="hint">
          This silo will only ever talk to a server presenting this key. If it changes, the
          connection stops rather than continuing to something else.
        </p>
        <button type="button" className="secondary" disabled={busy || checking} onClick={check}>
          Check again
        </button>
      </div>
    );
  }

  if (offered) {
    const changed = sftp.fingerprint && sftp.fingerprint !== offered;
    return (
      <div className="host-key">
        <div className="host-key-line">
          <KeyRound size={16} />
          <code>{offered}</code>
        </div>
        <p className="hint">
          {changed
            ? "This is not the key this silo has been using. Unless you rebuilt or moved the server, do not accept it."
            : "Compare this with the server itself before accepting it. Running ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub there prints the same line."}
        </p>
        <div className="host-key-actions">
          <button
            type="button"
            disabled={busy}
            onClick={() => {
              set({ fingerprint: offered });
              setOffered(null);
            }}
          >
            This is my server
          </button>
          <button type="button" className="secondary" disabled={busy} onClick={() => setOffered(null)}>
            Cancel
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="host-key">
      <button
        type="button"
        className="secondary"
        disabled={busy || checking || !sftp.host.trim()}
        onClick={check}
      >
        <ShieldCheck size={15} />
        {checking ? "Asking the server…" : "Check the server's identity"}
      </button>
      {error ? (
        <p className="hint is-error" role="status">
          {error}
        </p>
      ) : null}
      <p className="hint">
        Nothing is sent yet: no username, no password. The server offers its key first, and it is
        that key this silo will be tied to.
      </p>
    </div>
  );
}

type Props = {
  draft: StoreDraft;
  onChange: (draft: StoreDraft) => void;
  /** Whether a password is already stored, which changes what blank means. */
  hasStoredSecret: boolean;
  busy: boolean;
};

/**
 * Describing a place to keep a backup.
 *
 * Shared by Settings and by setting a silo up from an existing backup,
 * because those ask the identical question and had already drifted once —
 * the restore flow could only reach a bucket while Settings could reach
 * three kinds.
 */
export function StoreConfigForm({ draft, onChange, hasStoredSecret, busy }: Props) {
  const setKind = (kind: StoreKind) => onChange({ ...draft, kind });
  const setSftp = (patch: Partial<SftpForm>) =>
    onChange({ ...draft, sftp: { ...draft.sftp, ...patch } });

  return (
    <>
      <div className="field">
        <span>Where the backup lives</span>
        <div className="store-kind-picker">
          <button
            type="button"
            className={draft.kind === "s3" ? "" : "secondary"}
            onClick={() => setKind("s3")}
          >
            <Cloud size={15} />
            Bucket
          </button>
          <button
            type="button"
            className={draft.kind === "folder" ? "" : "secondary"}
            onClick={() => setKind("folder")}
          >
            <HardDrive size={15} />
            Folder
          </button>
          <button
            type="button"
            className={draft.kind === "web-dav" ? "" : "secondary"}
            onClick={() => setKind("web-dav")}
          >
            <Server size={15} />
            WebDAV
          </button>
          <button
            type="button"
            className={draft.kind === "sftp" ? "" : "secondary"}
            onClick={() => setKind("sftp")}
          >
            <Terminal size={15} />
            SFTP
          </button>
        </div>
      </div>

      {draft.kind === "folder" && (
        <label className="field">
          <span>Folder</span>
          <div className="path-picker">
            <input
              value={draft.folder}
              onChange={(e) => onChange({ ...draft, folder: e.target.value })}
              placeholder="\\nas\backups\silentsilo"
              spellCheck={false}
            />
            <button
              type="button"
              className="secondary"
              disabled={busy}
              onClick={() => {
                void openDialog({ directory: true, multiple: false }).then((picked) => {
                  if (typeof picked === "string") onChange({ ...draft, folder: picked });
                });
              }}
            >
              <FolderOpen size={15} />
              Browse
            </button>
          </div>
          <p className="hint">
            A network share, an external drive, or a folder your cloud client already syncs.
            Dropbox, OneDrive and Google Drive all work this way, and none of them see anything but
            ciphertext.
          </p>
        </label>
      )}

      {draft.kind === "web-dav" && (
        <>
          <label className="field">
            <span>Address</span>
            <input
              value={draft.dav.url}
              onChange={(e) => onChange({ ...draft, dav: { ...draft.dav, url: e.target.value } })}
              placeholder="https://cloud.example.com/remote.php/dav/files/you/silentsilo"
              spellCheck={false}
            />
            <p className="hint">
              The folder inside your Nextcloud, ownCloud, Synology or other WebDAV server. It will
              be created if it isn&apos;t there.
            </p>
            {isPlainHttp(draft.dav.url) && <p className="hint">{PLAIN_HTTP_WARNING}</p>}
          </label>
          <div className="s3-form-row">
            <label className="field">
              <span>Username</span>
              <input
                value={draft.dav.username}
                onChange={(e) =>
                  onChange({ ...draft, dav: { ...draft.dav, username: e.target.value } })
                }
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            <label className="field">
              <span>Password</span>
            <input
              type="password"
              value={draft.dav.password}
              onChange={(e) =>
                onChange({ ...draft, dav: { ...draft.dav, password: e.target.value } })
              }
              placeholder={hasStoredSecret ? "unchanged" : ""}
              autoComplete="off"
            />
              <p className="hint">
                On Nextcloud, generate an app password rather than using your account password.
                It can be revoked on its own.
              </p>
            </label>
          </div>
        </>
      )}

      {draft.kind === "sftp" && (
        <>
          <div className="sftp-host-row">
            <label className="field">
              <span>Server</span>
              <input
                value={draft.sftp.host}
                // A different machine means a different key, so the one
                // already accepted stops being an answer to this question.
                onChange={(e) => setSftp({ host: e.target.value, fingerprint: "" })}
                placeholder="nas.example.com"
                spellCheck={false}
              />
            </label>
            <label className="field">
              <span>Port</span>
              <input
                value={draft.sftp.port}
                onChange={(e) => setSftp({ port: e.target.value, fingerprint: "" })}
                inputMode="numeric"
                spellCheck={false}
              />
            </label>
          </div>

          <HostKeyStep sftp={draft.sftp} set={setSftp} busy={busy} />

          <div className="s3-form-row">
            <label className="field">
              <span>Username</span>
              <input
                value={draft.sftp.username}
                onChange={(e) => setSftp({ username: e.target.value })}
                spellCheck={false}
                autoComplete="off"
              />
            </label>

            <div className="field">
              <span>Sign in with</span>
            <div className="store-kind-picker">
              <button
                type="button"
                className={draft.sftp.method === "password" ? "" : "secondary"}
                onClick={() => setSftp({ method: "password" })}
              >
                Password
              </button>
                <button
                  type="button"
                  className={draft.sftp.method === "key" ? "" : "secondary"}
                  onClick={() => setSftp({ method: "key" })}
                >
                  <KeyRound size={15} />
                  Private key
                </button>
              </div>
            </div>
          </div>

          {draft.sftp.method === "password" ? (
            <label className="field">
              <span>Password</span>
              <input
                type="password"
                value={draft.sftp.password}
                onChange={(e) => setSftp({ password: e.target.value })}
                placeholder={hasStoredSecret ? "unchanged" : ""}
                autoComplete="off"
              />
            </label>
          ) : (
            <>
              <label className="field">
                <span>Private key</span>
                <textarea
                  value={draft.sftp.privateKey}
                  onChange={(e) => setSftp({ privateKey: e.target.value })}
                  placeholder={
                    hasStoredSecret ? "unchanged" : "-----BEGIN OPENSSH PRIVATE KEY-----"
                  }
                  spellCheck={false}
                  rows={4}
                />
                <p className="hint">
                  Paste the key itself, not a path to it. It is kept with the silo&apos;s other
                  secrets, so this keeps working if the file moves or the machine is rebuilt.
                </p>
              </label>
              <label className="field">
                <span>Key passphrase</span>
                <input
                  type="password"
                  value={draft.sftp.passphrase}
                  onChange={(e) => setSftp({ passphrase: e.target.value })}
                  placeholder={hasStoredSecret ? "unchanged" : "if the key has one"}
                  autoComplete="off"
                />
              </label>
            </>
          )}

          <label className="field">
            <span>Folder on the server</span>
            <input
              value={draft.sftp.path}
              onChange={(e) => setSftp({ path: e.target.value })}
              placeholder="backups/silentsilo"
              spellCheck={false}
            />
            <p className="hint">
              Relative to where you land when you log in, or an absolute path. It will be created
              if it isn&apos;t there.
            </p>
          </label>
        </>
      )}

      {draft.kind === "s3" && (
        <S3ConfigForm
          form={draft.s3}
          set={(key, value) => onChange({ ...draft, s3: { ...draft.s3, [key]: value } })}
          preset={draft.preset}
          choosePreset={(id) => {
            const next = S3_PRESETS.find((p) => p.id === id);
            if (!next) return;
            onChange({ ...draft, preset: next, s3: applyPreset(draft.s3, next) });
          }}
          connected={hasStoredSecret}
        />
      )}
    </>
  );
}
