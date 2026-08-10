import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { ChevronDown, ChevronRight, Paperclip } from "lucide-react";
import type { PasswordAttachment, PasswordCategory, PasswordEntry } from "../../lib/types";
import { parseTotpInput, DEFAULT_TOTP_ALGORITHM, DEFAULT_TOTP_DIGITS, DEFAULT_TOTP_PERIOD } from "../../lib/totp";
import { formatBytes } from "../../lib/format";
import { formatAppError } from "../../lib/errors";
import { TotpDisplay } from "./TotpDisplay";
import {
  categoryChoices,
  DEFAULT_GEN_OPTIONS,
  generatePassword,
  passwordStrength,
  TYPE_LABELS,
  typeOf,
  type PasswordGenOptions,
} from "./util";
import {
  IconClose,
  IconCopy,
  IconEye,
  IconEyeOff,
  IconGenerate,
  IconPlus,
  IconTrash,
} from "../../ui/Icons";

type Props = {
  /** The entry as it was when editing started. The editor owns its draft. */
  initial: PasswordEntry;
  creating: boolean;
  categories: PasswordCategory[];
  now: number;
  onSave: (entry: PasswordEntry) => void;
  onCancel: () => void;
};

/**
 * The create/edit form, filling the detail pane rather than a modal.
 *
 * The pane is where the entry is read, so it is also where it is changed:
 * a modal over the list hid the very entry being edited and capped the form
 * at dialog width, which is why the generator felt crowded.
 *
 * The generator's options fold away by default. The dice button already
 * covers the common case; the slider and character sets are for the site
 * with baroque password rules, and permanently spending five lines on them
 * made every edit look like work.
 */
export function EntryEditor({ initial, creating, categories, now, onSave, onCancel }: Props) {
  const [draft, setDraft] = useState<PasswordEntry>({ ...initial });
  const [passwordVisible, setPasswordVisible] = useState(false);
  const [genOptions, setGenOptions] = useState<PasswordGenOptions>(DEFAULT_GEN_OPTIONS);
  const [genOpen, setGenOpen] = useState(false);
  const [totpInput, setTotpInput] = useState(initial.totp_secret ?? "");
  const [totpError, setTotpError] = useState(false);
  const [attachBusy, setAttachBusy] = useState(false);
  const [attachError, setAttachError] = useState<string | null>(null);

  /// Secrets go through the Rust side rather than the webview's clipboard
  /// API: on Windows that keeps them out of Clipboard History, which writes
  /// to disk, and out of Cloud Clipboard, and clears them again after a
  /// minute or so.
  const copySecret = useCallback(async (text: string) => {
    await invoke("copy_secret_to_clipboard", { text });
  }, []);

  const applyTotpInput = useCallback((value: string) => {
    setTotpInput(value);
    if (!value.trim()) {
      setTotpError(false);
      setDraft((d) => ({
        ...d,
        totp_secret: undefined,
        totp_digits: undefined,
        totp_period: undefined,
        totp_algorithm: undefined,
      }));
      return;
    }
    const parsed = parseTotpInput(value);
    if (!parsed) {
      setTotpError(true);
      return;
    }
    setTotpError(false);
    setDraft((d) => ({
      ...d,
      totp_secret: parsed.secret,
      totp_digits: parsed.digits === DEFAULT_TOTP_DIGITS ? undefined : parsed.digits,
      totp_period: parsed.period === DEFAULT_TOTP_PERIOD ? undefined : parsed.period,
      totp_algorithm: parsed.algorithm === DEFAULT_TOTP_ALGORITHM ? undefined : parsed.algorithm,
    }));
  }, []);

  /// Blobs are written the moment a file is picked, so Cancel has cleanup
  /// to do: anything attached in this session but not saved would otherwise
  /// sit in the blob store with no reference anywhere.
  const initialIds = new Set((initial.attachments ?? []).map((a) => a.blob_id));

  const attachFiles = useCallback(async () => {
    setAttachError(null);
    const picked = await openFileDialog({ multiple: true });
    const paths = typeof picked === "string" ? [picked] : (picked ?? []);
    if (paths.length === 0) return;

    setAttachBusy(true);
    try {
      for (const path of paths) {
        const attachment = await invoke<PasswordAttachment>("password_attach_file", { path });
        setDraft((d) => ({ ...d, attachments: [...(d.attachments ?? []), attachment] }));
      }
    } catch (e) {
      setAttachError(formatAppError(e));
    } finally {
      setAttachBusy(false);
    }
  }, []);

  const removeAttachment = useCallback((blobId: string) => {
    // The reference goes now; the content goes on Save, so Cancel can still
    // put everything back exactly as it was.
    setDraft((d) => ({
      ...d,
      attachments: (d.attachments ?? []).filter((a) => a.blob_id !== blobId),
    }));
  }, []);

  const handleSave = useCallback(() => {
    const kept = new Set((draft.attachments ?? []).map((a) => a.blob_id));
    for (const a of initial.attachments ?? []) {
      if (!kept.has(a.blob_id)) {
        void invoke("password_delete_attachment", { blobId: a.blob_id }).catch(() => {});
      }
    }
    onSave(draft);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft, onSave]);

  const handleCancel = useCallback(() => {
    for (const a of draft.attachments ?? []) {
      if (!initialIds.has(a.blob_id)) {
        void invoke("password_delete_attachment", { blobId: a.blob_id }).catch(() => {});
      }
    }
    onCancel();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [draft, onCancel]);

  const strength = passwordStrength(draft.password);
  const type = typeOf(draft);
  const kind = TYPE_LABELS[type].singular.toLowerCase();

  const [sshBusy, setSshBusy] = useState(false);
  const [sshError, setSshError] = useState<string | null>(null);
  const generateSshKey = useCallback(async () => {
    setSshError(null);
    setSshBusy(true);
    try {
      const pair = await invoke<{ private_key: string; public_key: string; fingerprint: string }>(
        "ssh_generate_keypair"
      );
      setDraft((d) => ({
        ...d,
        ssh_private_key: pair.private_key,
        ssh_public_key: pair.public_key,
        ssh_fingerprint: pair.fingerprint,
      }));
    } catch (e) {
      setSshError(formatAppError(e));
    } finally {
      setSshBusy(false);
    }
  }, []);

  /// What the Add button is waiting for, named so a disabled button is an
  /// instruction rather than a mystery.
  const missingForSave: string[] = [];
  if (draft.service.trim().length === 0) {
    missingForSave.push(type === "login" ? "a service name" : "a name");
  }
  if (type === "login" && draft.password.trim().length === 0) {
    missingForSave.push("a password");
  }
  if (type === "card" && (draft.card_number ?? "").trim().length === 0) {
    missingForSave.push("the card number");
  }
  if (type === "identity" && (draft.id_full_name ?? "").trim().length === 0) {
    missingForSave.push("the full name");
  }
  if (type === "ssh_key" && (draft.ssh_private_key ?? "").trim().length === 0) {
    missingForSave.push("a private key");
  }
  const canSave = missingForSave.length === 0;

  const field = (
    label: string,
    key: keyof PasswordEntry,
    placeholder = "",
    full = false
  ) => (
    <label className={`field${full ? " field-full" : ""}`}>
      <span>{label}</span>
      <input
        type="text"
        placeholder={placeholder}
        value={(draft[key] as string | undefined) ?? ""}
        onChange={(e) => setDraft({ ...draft, [key]: e.target.value })}
      />
    </label>
  );

  return (
    <div className="pw-editor" role="form" aria-label={`${creating ? "Add" : "Edit"} ${kind}`}>
      <h3 className="pw-detail-heading">
        {creating ? "Add" : "Edit"} {kind}
      </h3>
      <div className="pw-form">
        <label className="field">
          <span>{type === "login" ? "Service" : "Name"}</span>
          <input
            type="text"
            placeholder={
              type === "login"
                ? "e.g. GitHub, Gmail"
                : type === "card"
                  ? "e.g. Personal Visa"
                  : type === "identity"
                    ? "e.g. Home address"
                    : "e.g. Work laptop key"
            }
            autoFocus
            value={draft.service}
            onChange={(e) => setDraft({ ...draft, service: e.target.value })}
          />
        </label>

        {type === "card" && (
          <>
            {field("Cardholder", "card_holder", "Name on the card")}
            {field("Number", "card_number", "1234 5678 9012 3456", true)}
            {field("Expiry month", "card_exp_month", "MM")}
            {field("Expiry year", "card_exp_year", "YYYY")}
            {field("Security code", "card_code", "CVC")}
            {field("Brand", "card_brand", "e.g. Visa")}
          </>
        )}

        {type === "identity" && (
          <>
            {field("Full name", "id_full_name", "First and last name")}
            {field("Company", "id_company")}
            {field("Email", "id_email")}
            {field("Phone", "id_phone")}
            {field("Address", "id_address", "Street and number", true)}
            {field("City", "id_city")}
            {field("State / County", "id_state")}
            {field("Postal code", "id_zip")}
            {field("Country", "id_country")}
          </>
        )}

        {type === "ssh_key" && (
          <>
            <div className="field field-full">
              <span>Key pair</span>
              {(draft.ssh_private_key ?? "").trim() === "" ? (
                <div className="pw-attachments">
                  <button
                    type="button"
                    className="pw-attach-btn"
                    disabled={sshBusy}
                    onClick={() => void generateSshKey()}
                  >
                    <IconGenerate size={14} />
                    <span>{sshBusy ? "Generating…" : "Generate an ed25519 key"}</span>
                  </button>
                </div>
              ) : (
                <p className="hint">
                  To generate a fresh pair, clear the private key below first.
                </p>
              )}
              {sshError && <p className="hint is-error">{sshError}</p>}
            </div>
            <label className="field field-full">
              <span>Private key</span>
              <textarea
                rows={5}
                placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
                spellCheck={false}
                value={draft.ssh_private_key ?? ""}
                onChange={(e) => setDraft({ ...draft, ssh_private_key: e.target.value })}
              />
            </label>
            <label className="field field-full">
              <span>Public key</span>
              <textarea
                rows={2}
                placeholder="ssh-ed25519 …"
                spellCheck={false}
                value={draft.ssh_public_key ?? ""}
                onChange={(e) => setDraft({ ...draft, ssh_public_key: e.target.value })}
              />
            </label>
            {field("Fingerprint", "ssh_fingerprint", "SHA256:…", true)}
          </>
        )}

        {type === "login" && (
          <>
        <label className="field">
          <span>Username / Email</span>
          <input
            type="text"
            placeholder="your@email.com"
            value={draft.username}
            onChange={(e) => setDraft({ ...draft, username: e.target.value })}
          />
        </label>
        <label className="field field-full">
          <span>Password</span>
          <div className="pw-password-input-row">
            <input
              type={passwordVisible ? "text" : "password"}
              value={draft.password}
              onChange={(e) => setDraft({ ...draft, password: e.target.value })}
            />
            <button
              type="button"
              className="pw-gen-btn"
              title={passwordVisible ? "Hide" : "Show"}
              onClick={() => setPasswordVisible((v) => !v)}
            >
              {passwordVisible ? <IconEyeOff size={15} /> : <IconEye size={15} />}
            </button>
            <button
              type="button"
              className="pw-gen-btn"
              title="Copy password"
              onClick={() => void copySecret(draft.password)}
            >
              <IconCopy size={15} />
            </button>
            <button
              type="button"
              className="pw-gen-btn accent"
              title="Generate new password"
              onClick={() => setDraft({ ...draft, password: generatePassword(genOptions) })}
            >
              <IconGenerate size={15} />
            </button>
          </div>

          <div className="pw-strength">
            <div className="pw-strength-track">
              <div
                className="pw-strength-fill"
                style={{ width: `${(strength.score / 4) * 100}%`, background: strength.color }}
              />
            </div>
            <span className="pw-strength-label" style={{ color: strength.color }}>
              {strength.label}
            </span>
            {/* `link` carries the full reset for the global button rule
                (background, shadow, min-height); without it this rendered
                as a dark unreadable capsule. */}
            <button
              type="button"
              className="link pw-gen-toggle"
              aria-expanded={genOpen}
              onClick={() => setGenOpen((v) => !v)}
            >
              {genOpen ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
              Generator options
            </button>
          </div>

          {genOpen && (
            <div className="pw-gen-panel">
              <div className="pw-gen-panel-header">
                <span>Generator</span>
                <span className="pw-gen-length-value">{genOptions.length} characters</span>
              </div>
              <input
                type="range"
                min={8}
                max={64}
                value={genOptions.length}
                onChange={(e) => setGenOptions({ ...genOptions, length: Number(e.target.value) })}
                className="pw-gen-slider"
              />
              <div className="pw-gen-chips">
                {(
                  [
                    ["upper", "A-Z"],
                    ["lower", "a-z"],
                    ["digits", "0-9"],
                    ["symbols", "!@#$"],
                  ] as const
                ).map(([key, label]) => (
                  <button
                    key={key}
                    type="button"
                    className={`pw-gen-chip${genOptions[key] ? " active" : ""}`}
                    onClick={() => setGenOptions({ ...genOptions, [key]: !genOptions[key] })}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
          )}
        </label>

        <label className="field field-full">
          <span>Authenticator (TOTP)</span>
          <div className="pw-totp-panel">
            {draft.totp_secret && !totpError ? (
              <>
                <TotpDisplay
                  entry={draft}
                  now={now}
                  copied={false}
                  onCopy={(code) => void copySecret(code)}
                />
                <button type="button" className="pw-totp-remove-btn" onClick={() => applyTotpInput("")}>
                  <IconClose size={13} />
                  <span>Remove</span>
                </button>
              </>
            ) : (
              <>
                <input
                  type="text"
                  placeholder="Secret key or otpauth:// link"
                  value={totpInput}
                  onChange={(e) => applyTotpInput(e.target.value)}
                  className={totpError ? "pw-input-error" : undefined}
                  autoComplete="off"
                  spellCheck={false}
                />
                <p className={`hint pw-totp-hint${totpError ? " pw-totp-hint-error" : ""}`}>
                  {totpError
                    ? "That is not a TOTP secret or an otpauth:// link."
                    : "Found under “can’t scan the QR code?” on the site’s 2FA setup page. Paste the text secret, or the whole otpauth:// link."}
                </p>
              </>
            )}
          </div>
        </label>

        <label className="field">
          <span>URL</span>
          <input
            type="url"
            placeholder="https://example.com"
            value={draft.url}
            onChange={(e) => setDraft({ ...draft, url: e.target.value })}
          />
        </label>
          </>
        )}

        <label className="field">
          <span>Category</span>
          <select
            value={draft.category}
            onChange={(e) => setDraft({ ...draft, category: e.target.value })}
          >
            {/* The entry's own category stays choosable even if it has been
                deleted from the list since; anything else would silently
                reassign the entry just by opening the editor. */}
            {[...new Set([...categoryChoices(categories), draft.category])]
              .filter(Boolean)
              .map((cat) => (
                <option key={cat} value={cat}>
                  {cat}
                </option>
              ))}
          </select>
        </label>

        <div className="field field-full">
          <span>Attached files</span>
          <div className="pw-attachments">
            {(draft.attachments ?? []).map((a) => (
              <div key={a.blob_id} className="pw-attachment-row">
                <Paperclip size={14} aria-hidden />
                <span className="pw-attachment-name">{a.name}</span>
                <span className="pw-attachment-size">{formatBytes(a.size_bytes)}</span>
                <button
                  type="button"
                  className="pw-inline-btn danger"
                  title="Remove file"
                  onClick={() => removeAttachment(a.blob_id)}
                >
                  <IconTrash size={13} />
                </button>
              </div>
            ))}
            <button
              type="button"
              className="pw-attach-btn"
              disabled={attachBusy}
              onClick={() => void attachFiles()}
            >
              <IconPlus size={14} />
              <span>{attachBusy ? "Encrypting…" : "Attach a file"}</span>
            </button>
          </div>
          {attachError && <p className="hint is-error">{attachError}</p>}
          <p className="hint">
            Encrypted and kept only with this entry. Attached files never appear in the file
            explorer.
          </p>
        </div>

        <div className="field field-full">
          <span>Protection</span>
          <label className="pw-reauth-toggle">
            <input
              type="checkbox"
              checked={draft.require_reauth ?? false}
              onChange={(e) =>
                setDraft({ ...draft, require_reauth: e.target.checked || undefined })
              }
            />
            <span>Ask for my security key or Windows Hello before showing this entry</span>
          </label>
          <p className="hint">
            Applies to revealing or copying the password, the one-time code, and opening attached
            files. One touch covers the next few minutes.
          </p>
        </div>

        <label className="field field-full">
          <span>Notes</span>
          <textarea
            rows={type === "note" ? 10 : 6}
            placeholder={
              type === "note" ? "The note itself" : "Anything else worth keeping with this entry"
            }
            value={draft.notes}
            onChange={(e) => setDraft({ ...draft, notes: e.target.value })}
          />
        </label>
      </div>
      <div className="pw-editor-actions">
        {!canSave && (
          <span className="hint">Still needed: {missingForSave.join(" and ")}.</span>
        )}
        <button type="button" className="secondary" onClick={handleCancel}>
          Cancel
        </button>
        <button
          type="button"
          disabled={!canSave}
          title={!canSave ? `Still needed: ${missingForSave.join(" and ")}.` : undefined}
          onClick={handleSave}
        >
          {creating ? "Add" : "Update"}
        </button>
      </div>
    </div>
  );
}
