import { useEffect, useState } from "react";
import {
  Cloud,
  CloudCog,
  FolderOpen,
  HardDrive,
  KeyRound,
  LogIn,
  Server,
  ShieldCheck,
  Terminal,
  UserCheck,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "../lib/dialog";
import { formatAppError } from "../lib/errors";
import {
  applyPreset,
  DEFAULT_S3_FORM,
  missingFields,
  S3_PRESETS,
  type S3Form,
  type S3Preset,
} from "../lib/s3Presets";
import {
  CLOUD_KINDS,
  isCloudKind,
  type CloudKind,
  type CloudSignIn,
  type StoreKind,
} from "../lib/types";
import {
  CLOUD_COMPANY,
  CLOUD_NAME,
  CLOUD_PLACE,
  cloudFolderProblem,
  DEFAULT_CLOUD_FOLDER,
} from "../lib/cloud";
import { formatBytes } from "../lib/format";
import { KDRIVE_DEFAULT_FOLDER, kdriveIdFrom, kdriveUrl } from "../lib/kdrive";
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

/**
 * A OneDrive, Dropbox or Google Drive copy. The sign-in itself stays in the
 * app's Rust side; this holds only its id and what to show about it.
 */
export type CloudForm = {
  /** The sign-in to save the copy with; null keeps the stored one. */
  signIn: string | null;
  /** The account shown, from the sign-in or from the stored copy. */
  account: string;
  freeBytes: number | null;
  folder: string;
};

const EMPTY_CLOUD_FORM: CloudForm = {
  signIn: null,
  account: "",
  freeBytes: null,
  folder: DEFAULT_CLOUD_FOLDER,
};

/**
 * A WebDAV server, typed as an address, or kDrive, whose address is built
 * from the drive's ID and a folder.
 */
export type DavForm = {
  preset: "any" | "kdrive";
  url: string;
  username: string;
  password: string;
  kdriveId: string;
  kdriveFolder: string;
};

export type StoreDraft = {
  kind: StoreKind;
  preset: S3Preset;
  s3: S3Form;
  folder: string;
  dav: DavForm;
  sftp: SftpForm;
  cloud: Record<CloudKind, CloudForm>;
};

/// Starts on a folder: the choice most people can make without an account
/// anywhere, and the one with a single field.
export const EMPTY_STORE_DRAFT: StoreDraft = {
  kind: "folder",
  preset: S3_PRESETS[S3_PRESETS.length - 1]!,
  s3: DEFAULT_S3_FORM,
  folder: "",
  dav: {
    preset: "any",
    url: "",
    username: "",
    password: "",
    kdriveId: "",
    kdriveFolder: KDRIVE_DEFAULT_FOLDER,
  },
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
  cloud: {
    onedrive: EMPTY_CLOUD_FORM,
    dropbox: EMPTY_CLOUD_FORM,
    "google-drive": EMPTY_CLOUD_FORM,
  },
};

/** What the backend expects, for whichever kind is selected. */
export function storeDraftPayload(draft: StoreDraft) {
  if (isCloudKind(draft.kind)) {
    const cloud = draft.cloud[draft.kind];
    return { kind: draft.kind, signIn: cloud.signIn, folder: cloud.folder.trim() };
  }
  switch (draft.kind) {
    case "folder":
      return { kind: "folder" as const, path: draft.folder };
    case "web-dav":
      return {
        kind: "web-dav" as const,
        url:
          draft.dav.preset === "kdrive"
            ? kdriveUrl(draft.dav.kdriveId, draft.dav.kdriveFolder)
            : draft.dav.url,
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
  if (isCloudKind(draft.kind)) {
    const cloud = draft.cloud[draft.kind];
    return [
      !cloud.signIn && !cloud.account && `a ${CLOUD_NAME[draft.kind]} sign-in`,
      !cloud.folder.trim() && "folder name",
    ].filter((v): v is string => typeof v === "string");
  }
  switch (draft.kind) {
    case "folder":
      return draft.folder.trim() ? [] : ["folder"];
    case "web-dav":
      return [
        draft.dav.preset === "kdrive"
          ? !kdriveIdFrom(draft.dav.kdriveId) && "kDrive ID"
          : !draft.dav.url.trim() && "address",
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
          SilentSilo connects only to a server with this key. If the key changes, the connection
          stops.
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
        Your username and password are not sent at this step. The server shows its key first, and
        this silo is tied to that key.
      </p>
    </div>
  );
}

/**
 * Signing in to the provider, in the user's own browser.
 *
 * The app never sees the password or the second factor: the provider's
 * page opens in the default browser, and what comes back is the account to
 * show and an id the save refers to. Setting up from backup storage then
 * lists the silo folders the account holds instead of asking for a name.
 */
function CloudStep({
  kind,
  form,
  set,
  busy,
  joining,
}: {
  kind: CloudKind;
  form: CloudForm;
  set: (patch: Partial<CloudForm>) => void;
  busy: boolean;
  joining: boolean;
}) {
  const [signingIn, setSigningIn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [found, setFound] = useState<string[] | null>(null);

  const signIn = async () => {
    setSigningIn(true);
    setError(null);
    setFound(null);
    try {
      const done = await invoke<CloudSignIn>("cloud_sign_in", { kind });
      // One update: `set` closes over the draft of the render this started
      // in, so a second call would undo the first.
      const folders = joining
        ? await invoke<string[]>("cloud_list_silos", { signIn: done.id })
        : null;
      setFound(folders);
      set({
        signIn: done.id,
        account: done.account.label,
        freeBytes: done.account.freeBytes,
        ...(folders && folders.length > 0 ? { folder: folders[0]! } : {}),
      });
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setSigningIn(false);
    }
  };

  const folderProblem = form.folder ? cloudFolderProblem(form.folder) : null;

  return (
    <>
      <div className="host-key">
        {form.account && !signingIn ? (
          <>
            <div className="host-key-line">
              <UserCheck size={16} />
              <span>
                Connected as <strong>{form.account}</strong>
                {form.freeBytes !== null ? `, ${formatBytes(form.freeBytes)} free` : ""}
              </span>
            </div>
            <button
              type="button"
              className="secondary"
              disabled={busy}
              onClick={() => void signIn()}
            >
              Use another account
            </button>
          </>
        ) : signingIn ? (
          <>
            <p className="hint" role="status">
              <span className="spinner" aria-hidden /> Finish signing in to {CLOUD_NAME[kind]} in
              your browser, then come back here.
            </p>
            <button
              type="button"
              className="secondary"
              onClick={() => void invoke("cloud_cancel_sign_in").catch(() => {})}
            >
              Cancel
            </button>
          </>
        ) : (
          <>
            <button type="button" disabled={busy} onClick={() => void signIn()}>
              <LogIn size={15} />
              Connect {CLOUD_NAME[kind]}
            </button>
            <p className="hint">
              Opens {CLOUD_COMPANY[kind]}&apos;s sign-in page in your browser. SilentSilo never
              sees your password, and gets access only to its own folder.
            </p>
          </>
        )}
        {error ? (
          <p className="hint is-error" role="status">
            {error}
          </p>
        ) : null}
      </div>

      {joining && found !== null ? (
        found.length > 0 ? (
          <label className="field">
            <span>Silo folder</span>
            <select
              value={form.folder}
              disabled={busy}
              onChange={(e) => set({ folder: e.target.value })}
            >
              {found.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
            <p className="hint">The folders in {CLOUD_PLACE[kind]}.</p>
          </label>
        ) : (
          <p className="hint is-error" role="status">
            There is no silo in {CLOUD_PLACE[kind]} yet. Sync once from the computer that has it.
          </p>
        )
      ) : !joining ? (
        <label className="field">
          <span>Folder name</span>
          <input
            value={form.folder}
            disabled={busy}
            onChange={(e) => set({ folder: e.target.value })}
            spellCheck={false}
          />
          <p className={`hint${folderProblem ? " is-error" : ""}`}>
            {folderProblem ??
              `In ${CLOUD_PLACE[kind]}. ${CLOUD_COMPANY[kind]} sees this name. Your files inside are encrypted.`}
          </p>
        </label>
      ) : null}
    </>
  );
}

type Props = {
  draft: StoreDraft;
  onChange: (draft: StoreDraft) => void;
  /** Whether a password is already stored, which changes what blank means. */
  hasStoredSecret: boolean;
  busy: boolean;
  /** Setting up from backup storage: a cloud account lists its silos. */
  joining?: boolean;
};

/**
 * Describing a place to keep a backup.
 *
 * Shared by Settings and by setting a silo up from an existing backup,
 * because those ask the identical question and had already drifted once —
 * the restore flow could only reach a bucket while Settings could reach
 * three kinds.
 */
export function StoreConfigForm({ draft, onChange, hasStoredSecret, busy, joining }: Props) {
  const setKind = (kind: StoreKind) => onChange({ ...draft, kind });
  const setSftp = (patch: Partial<SftpForm>) =>
    onChange({ ...draft, sftp: { ...draft.sftp, ...patch } });
  const setDav = (patch: Partial<DavForm>) => onChange({ ...draft, dav: { ...draft.dav, ...patch } });
  // A build without a provider's client details leaves it out.
  const [clouds, setClouds] = useState<CloudKind[]>([]);
  useEffect(() => {
    let live = true;
    void invoke<string[]>("cloud_providers")
      .then((kinds) => {
        if (live) setClouds(CLOUD_KINDS.filter((k) => kinds.includes(k)));
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  return (
    <>
      <div className="field">
        <span>Where should the encrypted copy live?</span>
        <div className="store-kind-picker">
          <button
            type="button"
            className={draft.kind === "folder" ? "" : "secondary"}
            onClick={() => setKind("folder")}
          >
            <HardDrive size={15} />A drive or NAS folder
          </button>
          <button
            type="button"
            className={draft.kind === "s3" ? "" : "secondary"}
            onClick={() => setKind("s3")}
          >
            <Cloud size={15} />A cloud bucket (S3)
          </button>
          <button
            type="button"
            className={draft.kind === "web-dav" ? "" : "secondary"}
            onClick={() => setKind("web-dav")}
          >
            <Server size={15} />
            Nextcloud or WebDAV
          </button>
          <button
            type="button"
            className={draft.kind === "sftp" ? "" : "secondary"}
            onClick={() => setKind("sftp")}
          >
            <Terminal size={15} />A server over SFTP
          </button>
          {clouds.map((kind) => (
            <button
              key={kind}
              type="button"
              className={draft.kind === kind ? "" : "secondary"}
              onClick={() => setKind(kind)}
            >
              <CloudCog size={15} />
              {CLOUD_NAME[kind]}
            </button>
          ))}
        </div>
      </div>

      {isCloudKind(draft.kind) && (
        <CloudStep
          kind={draft.kind}
          form={draft.cloud[draft.kind]}
          set={(patch) => {
            const kind = draft.kind as CloudKind;
            onChange({
              ...draft,
              cloud: { ...draft.cloud, [kind]: { ...draft.cloud[kind], ...patch } },
            });
          }}
          busy={busy}
          joining={joining ?? false}
        />
      )}

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
            A network share, an external drive, or a folder that Dropbox, OneDrive or Google Drive
            already syncs. What SilentSilo writes there is encrypted.
          </p>
        </label>
      )}

      {draft.kind === "web-dav" && (
        <>
          <div className="field">
            <span>Server</span>
            <div className="store-kind-picker">
              <button
                type="button"
                className={draft.dav.preset === "any" ? "" : "secondary"}
                onClick={() => setDav({ preset: "any" })}
              >
                Any WebDAV server
              </button>
              <button
                type="button"
                className={draft.dav.preset === "kdrive" ? "" : "secondary"}
                onClick={() => setDav({ preset: "kdrive" })}
              >
                kDrive (Infomaniak)
              </button>
            </div>
          </div>
          {draft.dav.preset === "kdrive" ? (
            <div className="s3-form-row">
              <label className="field">
                <span>kDrive ID</span>
                <input
                  value={draft.dav.kdriveId}
                  onChange={(e) => setDav({ kdriveId: kdriveIdFrom(e.target.value) })}
                  placeholder="123456"
                  inputMode="numeric"
                  spellCheck={false}
                />
                <p className="hint">
                  The number after /drive/ in the address bar when kDrive is open in your
                  browser.
                </p>
              </label>
              <label className="field">
                <span>Folder in kDrive</span>
                <input
                  value={draft.dav.kdriveFolder}
                  onChange={(e) => setDav({ kdriveFolder: e.target.value })}
                  spellCheck={false}
                />
                <p className="hint">Created if it does not exist.</p>
              </label>
            </div>
          ) : (
            <label className="field">
              <span>Address</span>
              <input
                value={draft.dav.url}
                onChange={(e) => onChange({ ...draft, dav: { ...draft.dav, url: e.target.value } })}
                placeholder="https://cloud.example.com/remote.php/dav/files/you/silentsilo"
                spellCheck={false}
              />
              <p className="hint">
                The folder inside your Nextcloud, ownCloud, Synology or other WebDAV server. It is
                created if it does not exist.
              </p>
              {isPlainHttp(draft.dav.url) && <p className="hint">{PLAIN_HTTP_WARNING}</p>}
            </label>
          )}
          <div className="s3-form-row">
            <label className="field">
              <span>{draft.dav.preset === "kdrive" ? "Infomaniak email" : "Username"}</span>
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
                {draft.dav.preset === "kdrive"
                  ? "An app password from your Infomaniak profile, not your account password. It is required when the account has two-step sign-in, and you can revoke it on its own."
                  : "On Nextcloud, use an app password, not your account password. You can revoke it on its own."}
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
                  Paste the key itself, not a path to it. SilentSilo keeps it with the
                  silo&apos;s other sign-in details.
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
              Relative to where you land when you log in, or an absolute path. It is created if it
              does not exist.
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
