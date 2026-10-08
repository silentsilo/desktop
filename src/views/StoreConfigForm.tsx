import { useEffect, useRef, useState } from "react";
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
  cloudFolderHint,
  cloudFolderProblem,
  cloudFoldersHint,
  cloudNoSilo,
  DEFAULT_CLOUD_FOLDER,
} from "../lib/cloud";
import { formatBytes } from "../lib/format";
import { KDRIVE_DEFAULT_FOLDER, kdriveIdFrom, kdriveUrl } from "../lib/kdrive";
import { S3ConfigForm } from "./S3ConfigForm";
import { plainHttpWarning, isPlainHttp } from "../lib/plainHttp";
import { t, tx, useLocale, type Key } from "../i18n";

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

/**
 * Lets go of the sign-ins a form holds and never saved: on Cancel or Back.
 * Their tokens otherwise wait in the app until it locks; a Dropbox one is
 * also ended at Dropbox. One a saved copy adopted is gone already, so this
 * is safe to call after a save as well.
 */
export function discardSignIns(draft: StoreDraft): void {
  for (const form of Object.values(draft.cloud)) {
    if (form.signIn) {
      void invoke("cloud_discard_sign_in", { signIn: form.signIn }).catch(() => {});
    }
  }
}

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
      !cloud.signIn &&
        !cloud.account &&
        t("backup.missing_cloud_sign_in", { provider: CLOUD_NAME[draft.kind] }),
      !cloud.folder.trim() && t("backup.missing_folder_name"),
    ].filter((v): v is string => typeof v === "string");
  }
  switch (draft.kind) {
    case "folder":
      return draft.folder.trim() ? [] : [t("backup.missing_folder")];
    case "web-dav":
      return [
        draft.dav.preset === "kdrive"
          ? !kdriveIdFrom(draft.dav.kdriveId) && t("backup.kdrive_id")
          : !draft.dav.url.trim() && t("backup.missing_address"),
        !draft.dav.username.trim() && t("backup.missing_username"),
        !draft.dav.password.trim() && !hasStoredSecret && t("backup.word_password"),
      ].filter((v): v is string => typeof v === "string");
    case "sftp":
      return [
        !draft.sftp.host.trim() && t("backup.missing_address"),
        !draft.sftp.username.trim() && t("backup.missing_username"),
        draft.sftp.method === "password"
          ? !draft.sftp.password.trim() && !hasStoredSecret && t("backup.word_password")
          : !draft.sftp.privateKey.trim() && !hasStoredSecret && t("backup.word_private_key"),
        !draft.sftp.fingerprint.trim() && !hasStoredSecret && t("backup.missing_fingerprint"),
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
  useLocale();
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
        <p className="hint">{t("backup.host_key_pinned")}</p>
        <button type="button" className="btn-secondary" disabled={busy || checking} onClick={check}>
          {t("backup.host_key_check_again")}
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
            ? t("backup.host_key_changed")
            : t("backup.host_key_compare", {
                command: "ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub",
              })}
        </p>
        <div className="host-key-actions">
          <button
            className="btn-primary"
            type="button"
            disabled={busy}
            onClick={() => {
              set({ fingerprint: offered });
              setOffered(null);
            }}
          >
            {t("backup.host_key_accept")}
          </button>
          <button
            type="button"
            className="btn-secondary"
            disabled={busy}
            onClick={() => setOffered(null)}
          >
            {t("common.cancel")}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="host-key">
      <button
        type="button"
        className="btn-secondary"
        disabled={busy || checking || !sftp.host.trim()}
        onClick={check}
      >
        <ShieldCheck size={15} />
        {checking ? t("backup.host_key_asking") : t("backup.host_key_check")}
      </button>
      {error ? (
        <p className="hint is-error" role="status">
          {error}
        </p>
      ) : null}
      <p className="hint">{t("backup.host_key_hint")}</p>
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
  useLocale();
  const [signingIn, setSigningIn] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [found, setFound] = useState<string[] | null>(null);

  const signIn = async () => {
    setSigningIn(true);
    setError(null);
    setFound(null);
    try {
      const done = await invoke<CloudSignIn>("cloud_sign_in", { kind });
      // Signed in again: the one it replaces is no longer going anywhere.
      if (form.signIn && form.signIn !== done.id) {
        void invoke("cloud_discard_sign_in", { signIn: form.signIn }).catch(() => {});
      }
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
                {form.freeBytes !== null
                  ? tx("backup.cloud_connected_as_free", {
                      account: <strong>{form.account}</strong>,
                      size: formatBytes(form.freeBytes),
                    })
                  : tx("backup.cloud_connected_as", { account: <strong>{form.account}</strong> })}
              </span>
            </div>
            <button
              type="button"
              className="btn-secondary"
              disabled={busy}
              onClick={() => void signIn()}
            >
              {t("backup.cloud_other_account")}
            </button>
          </>
        ) : signingIn ? (
          <>
            <p className="hint" role="status">
              <span className="spinner" aria-hidden />{" "}
              {t("backup.cloud_finish_sign_in", { provider: CLOUD_NAME[kind] })}
            </p>
            <button
              type="button"
              className="btn-secondary"
              onClick={() => void invoke("cloud_cancel_sign_in").catch(() => {})}
            >
              {t("common.cancel")}
            </button>
          </>
        ) : (
          <>
            <button
              className="btn-primary"
              type="button"
              disabled={busy}
              onClick={() => void signIn()}
            >
              <LogIn size={15} />
              {t("backup.cloud_connect", { provider: CLOUD_NAME[kind] })}
            </button>
            <p className="hint">
              {t("backup.cloud_connect_hint", { company: CLOUD_COMPANY[kind] })}
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
            <span>{t("backup.cloud_silo_folder")}</span>
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
            <p className="hint">{cloudFoldersHint(kind)}</p>
          </label>
        ) : (
          <p className="hint is-error" role="status">
            {cloudNoSilo(kind)}
          </p>
        )
      ) : !joining ? (
        <label className="field">
          <span>{t("backup.cloud_folder_name")}</span>
          <input
            value={form.folder}
            disabled={busy}
            onChange={(e) => set({ folder: e.target.value })}
            spellCheck={false}
          />
          <p className={`hint${folderProblem ? " is-error" : ""}`}>
            {folderProblem ?? cloudFolderHint(kind)}
          </p>
        </label>
      ) : null}
    </>
  );
}

/** The kinds that need no sign-in, in the order they are offered. */
const OWN_STORAGE: readonly { kind: StoreKind; label: Key; title: Key; Icon: typeof HardDrive }[] = [
  {
    kind: "folder",
    label: "backup.kind_folder_button",
    title: "backup.kind_folder_title",
    Icon: HardDrive,
  },
  { kind: "s3", label: "backup.kind_s3", title: "backup.kind_s3_title", Icon: Cloud },
  { kind: "web-dav", label: "backup.kind_webdav", title: "backup.kind_webdav_title", Icon: Server },
  { kind: "sftp", label: "backup.kind_sftp", title: "backup.kind_sftp_title", Icon: Terminal },
];

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
  useLocale();
  const latest = useRef(draft);
  latest.current = draft;
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
        <span>{t("backup.where_question")}</span>
        {clouds.length > 0 && (
          <>
            <p className="store-kind-group">{t("backup.group_accounts")}</p>
            <div className="store-kind-grid is-accounts">
              {clouds.map((kind) => (
                <button
                  key={kind}
                  type="button"
                  className={draft.kind === kind ? "btn-primary" : "btn-secondary"}
                  onClick={() => setKind(kind)}
                >
                  <CloudCog size={15} />
                  {CLOUD_NAME[kind]}
                </button>
              ))}
            </div>
            <p className="store-kind-group">{t("backup.group_own")}</p>
          </>
        )}
        <div className="store-kind-grid">
          {OWN_STORAGE.map(({ kind, label, title, Icon }) => (
            <button
              key={kind}
              type="button"
              title={t(title)}
              className={draft.kind === kind ? "btn-primary" : "btn-secondary"}
              onClick={() => setKind(kind)}
            >
              <Icon size={15} />
              {t(label)}
            </button>
          ))}
        </div>
      </div>

      {isCloudKind(draft.kind) && (
        <CloudStep
          kind={draft.kind}
          form={draft.cloud[draft.kind]}
          set={(patch) => {
            // The draft as it is now: a sign-in finishing later must not
            // undo what was typed meanwhile.
            const current = latest.current;
            const kind = current.kind as CloudKind;
            onChange({
              ...current,
              cloud: { ...current.cloud, [kind]: { ...current.cloud[kind], ...patch } },
            });
          }}
          busy={busy}
          joining={joining ?? false}
        />
      )}

      {draft.kind === "folder" && (
        <label className="field">
          <span>{t("backup.field_folder")}</span>
          <div className="path-picker">
            <input
              value={draft.folder}
              onChange={(e) => onChange({ ...draft, folder: e.target.value })}
              placeholder="\\nas\backups\silentsilo"
              spellCheck={false}
            />
            <button
              type="button"
              className="btn-secondary"
              disabled={busy}
              onClick={() => {
                void openDialog({ directory: true, multiple: false }).then((picked) => {
                  if (typeof picked === "string") onChange({ ...draft, folder: picked });
                });
              }}
            >
              <FolderOpen size={15} />
              {t("backup.browse")}
            </button>
          </div>
          <p className="hint">{t("backup.folder_hint")}</p>
        </label>
      )}

      {draft.kind === "web-dav" && (
        <>
          <div className="field">
            <span>{t("backup.field_server")}</span>
            <div className="store-kind-picker">
              <button
                type="button"
                className={draft.dav.preset === "any" ? "btn-primary" : "btn-secondary"}
                onClick={() => setDav({ preset: "any" })}
              >
                {t("backup.dav_any")}
              </button>
              <button
                type="button"
                className={draft.dav.preset === "kdrive" ? "btn-primary" : "btn-secondary"}
                onClick={() => setDav({ preset: "kdrive" })}
              >
                kDrive (Infomaniak)
              </button>
            </div>
          </div>
          {draft.dav.preset === "kdrive" ? (
            <div className="s3-form-row">
              <label className="field">
                <span>{t("backup.kdrive_id")}</span>
                <input
                  value={draft.dav.kdriveId}
                  onChange={(e) => setDav({ kdriveId: kdriveIdFrom(e.target.value) })}
                  placeholder="123456"
                  inputMode="numeric"
                  spellCheck={false}
                />
                <p className="hint">{t("backup.kdrive_id_hint")}</p>
              </label>
              <label className="field">
                <span>{t("backup.kdrive_folder")}</span>
                <input
                  value={draft.dav.kdriveFolder}
                  onChange={(e) => setDav({ kdriveFolder: e.target.value })}
                  spellCheck={false}
                />
                <p className="hint">{t("backup.created_if_missing")}</p>
              </label>
            </div>
          ) : (
            <label className="field">
              <span>{t("backup.field_address")}</span>
              <input
                value={draft.dav.url}
                onChange={(e) => onChange({ ...draft, dav: { ...draft.dav, url: e.target.value } })}
                placeholder="https://cloud.example.com/remote.php/dav/files/you/silentsilo"
                spellCheck={false}
              />
              <p className="hint">{t("backup.dav_address_hint")}</p>
              {isPlainHttp(draft.dav.url) && <p className="hint">{plainHttpWarning()}</p>}
            </label>
          )}
          <div className="s3-form-row">
            <label className="field">
              <span>
                {draft.dav.preset === "kdrive"
                  ? t("backup.infomaniak_email")
                  : t("backup.field_username")}
              </span>
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
              <span>{t("backup.field_password")}</span>
            <input
              type="password"
              value={draft.dav.password}
              onChange={(e) =>
                onChange({ ...draft, dav: { ...draft.dav, password: e.target.value } })
              }
              placeholder={hasStoredSecret ? t("backup.unchanged") : ""}
              autoComplete="off"
            />
              <p className="hint">
                {draft.dav.preset === "kdrive"
                  ? t("backup.kdrive_password_hint")
                  : t("backup.dav_password_hint")}
              </p>
            </label>
          </div>
        </>
      )}

      {draft.kind === "sftp" && (
        <>
          <div className="sftp-host-row">
            <label className="field">
              <span>{t("backup.field_server")}</span>
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
              <span>{t("backup.field_port")}</span>
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
              <span>{t("backup.field_username")}</span>
              <input
                value={draft.sftp.username}
                onChange={(e) => setSftp({ username: e.target.value })}
                spellCheck={false}
                autoComplete="off"
              />
            </label>

            <div className="field">
              <span>{t("backup.sign_in_with")}</span>
            <div className="store-kind-picker">
              <button
                type="button"
                className={draft.sftp.method === "password" ? "btn-primary" : "btn-secondary"}
                onClick={() => setSftp({ method: "password" })}
              >
                {t("backup.field_password")}
              </button>
                <button
                  type="button"
                  className={draft.sftp.method === "key" ? "btn-primary" : "btn-secondary"}
                  onClick={() => setSftp({ method: "key" })}
                >
                  <KeyRound size={15} />
                  {t("backup.private_key_label")}
                </button>
              </div>
            </div>
          </div>

          {draft.sftp.method === "password" ? (
            <label className="field">
              <span>{t("backup.field_password")}</span>
              <input
                type="password"
                value={draft.sftp.password}
                onChange={(e) => setSftp({ password: e.target.value })}
                placeholder={hasStoredSecret ? t("backup.unchanged") : ""}
                autoComplete="off"
              />
            </label>
          ) : (
            <>
              <label className="field">
                <span>{t("backup.private_key_label")}</span>
                <textarea
                  value={draft.sftp.privateKey}
                  onChange={(e) => setSftp({ privateKey: e.target.value })}
                  placeholder={
                    hasStoredSecret ? t("backup.unchanged") : "-----BEGIN OPENSSH PRIVATE KEY-----"
                  }
                  spellCheck={false}
                  rows={4}
                />
                <p className="hint">{t("backup.private_key_hint")}</p>
              </label>
              <label className="field">
                <span>{t("backup.passphrase")}</span>
                <input
                  type="password"
                  value={draft.sftp.passphrase}
                  onChange={(e) => setSftp({ passphrase: e.target.value })}
                  placeholder={
                    hasStoredSecret ? t("backup.unchanged") : t("backup.passphrase_placeholder")
                  }
                  autoComplete="off"
                />
              </label>
            </>
          )}

          <label className="field">
            <span>{t("backup.sftp_folder")}</span>
            <input
              value={draft.sftp.path}
              onChange={(e) => setSftp({ path: e.target.value })}
              placeholder="backups/silentsilo"
              spellCheck={false}
            />
            <p className="hint">{t("backup.sftp_folder_hint")}</p>
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
