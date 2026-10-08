import { useCallback, useState } from "react";
import { platformStrings, type Os } from "../../lib/platformStrings";
import { invoke } from "@tauri-apps/api/core";
import { open as openFileDialog } from "../../lib/dialog";
import { ChevronDown, ChevronRight, Paperclip } from "lucide-react";
import type {
  CustomField,
  PasswordAttachment,
  PasswordCategory,
  PasswordEntry,
} from "../../lib/types";
import { parseTotpInput, DEFAULT_TOTP_ALGORITHM, DEFAULT_TOTP_DIGITS, DEFAULT_TOTP_PERIOD } from "../../lib/totp";
import { formatBytes } from "../../lib/format";
import { formatAppError } from "../../lib/errors";
import { TotpDisplay } from "./TotpDisplay";
import {
  categoryChoices,
  DEFAULT_GEN_OPTIONS,
  generatePassword,
  passwordStrength,
  TYPE_TEXTS,
  typeOf,
  type PasswordGenOptions,
} from "./util";
import { SshAgentOption } from "./SshAgentOption";
import {
  IconClose,
  IconCopy,
  IconEye,
  IconEyeOff,
  IconGenerate,
  IconPlus,
  IconTrash,
} from "../../ui/Icons";
import { t, useLocale } from "../../i18n";

type Props = {
  /** The entry as it was when editing started. The editor owns its draft. */
  initial: PasswordEntry;
  /** Names the built-in authenticator in the re-verify option. */
  os: Os;
  creating: boolean;
  categories: PasswordCategory[];
  now: number;
  /** Resolves to whether the entry was stored. */
  onSave: (entry: PasswordEntry) => Promise<boolean>;
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
export function EntryEditor({ os, initial, creating, categories, now, onSave, onCancel }: Props) {
  useLocale();
  const [draft, setDraft] = useState<PasswordEntry>({ ...initial });
  const [passwordVisible, setPasswordVisible] = useState(false);
  const [genOptions, setGenOptions] = useState<PasswordGenOptions>(DEFAULT_GEN_OPTIONS);
  const [genOpen, setGenOpen] = useState(false);
  const [totpInput, setTotpInput] = useState(initial.totp_secret ?? "");
  const [totpError, setTotpError] = useState(false);
  /// The field stays a field while it has focus. It used to turn into the
  /// code display on the first valid character, so a secret could be pasted
  /// but not typed.
  const [totpTyping, setTotpTyping] = useState(false);
  const [attachBusy, setAttachBusy] = useState(false);
  const [attachError, setAttachError] = useState<string | null>(null);

  /// Secrets go through the Rust side rather than the webview's clipboard
  /// API: on Windows that keeps them out of Clipboard History, which writes
  /// to disk, and out of Cloud Clipboard, and clears them again after a
  /// minute or so.
  const copySecret = useCallback(
    async (text: string, field: string) => {
      await invoke("copy_secret_to_clipboard", {
        text,
        audit: { entry_id: draft.id, label: draft.service, field },
      });
    },
    [draft.id, draft.service],
  );

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

  /// Removed attachments lose their content only once the entry that no
  /// longer points at them is stored. Deleted first, a failed save left the
  /// stored entry pointing at content that was gone.
  const handleSave = useCallback(async () => {
    // A row left with neither a name nor a value is one the user added and
    // never used. None left means no `fields` at all, as before 1.4.
    const fields = (draft.fields ?? []).filter((f) => f.name.trim() || f.value);
    if (!(await onSave({ ...draft, fields: fields.length > 0 ? fields : undefined }))) return;
    const kept = new Set((draft.attachments ?? []).map((a) => a.blob_id));
    for (const a of initial.attachments ?? []) {
      if (!kept.has(a.blob_id)) {
        void invoke("password_delete_attachment", { blobId: a.blob_id }).catch(() => {});
      }
    }
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
  const heading = t(creating ? TYPE_TEXTS[type].add : TYPE_TEXTS[type].edit);

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
    missingForSave.push(type === "login" ? t("pw.need_service_name") : t("pw.need_name"));
  }
  // A login saved with a passkey from the phone signs in without a password.
  const hasPasskey = Boolean((draft as Record<string, unknown>).passkey);
  if (type === "login" && draft.password.trim().length === 0 && !hasPasskey) {
    missingForSave.push(t("pw.need_password"));
  }
  if (type === "card" && (draft.card_number ?? "").trim().length === 0) {
    missingForSave.push(t("pw.need_card_number"));
  }
  if (type === "identity" && (draft.id_full_name ?? "").trim().length === 0) {
    missingForSave.push(t("pw.need_full_name"));
  }
  if (type === "ssh_key" && (draft.ssh_private_key ?? "").trim().length === 0) {
    missingForSave.push(t("pw.need_private_key"));
  }
  const canSave = missingForSave.length === 0;
  // At most two: the name, and the one secret this kind needs.
  const stillNeeded =
    missingForSave.length === 1
      ? t("pw.still_needed_one", { what: missingForSave[0]! })
      : t("pw.still_needed_two", {
          first: missingForSave[0] ?? "",
          second: missingForSave.slice(1).join(", "),
        });

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
        autoComplete="off"
        placeholder={placeholder}
        value={(draft[key] as string | undefined) ?? ""}
        onChange={(e) => setDraft({ ...draft, [key]: e.target.value })}
      />
    </label>
  );

  return (
    <div className="pw-editor" role="form" aria-label={heading}>
      <h3 className="pw-detail-heading">{heading}</h3>
      <div className="pw-form">
        <label className="field">
          <span>{type === "login" ? t("pw.field_service") : t("pw.field_name")}</span>
          <input
            type="text"
            autoComplete="off"
            placeholder={
              type === "login"
                ? t("pw.placeholder_login")
                : type === "card"
                  ? t("pw.placeholder_card")
                  : type === "identity"
                    ? t("pw.placeholder_identity")
                    : t("pw.placeholder_other")
            }
            autoFocus
            value={draft.service}
            onChange={(e) => setDraft({ ...draft, service: e.target.value })}
          />
        </label>

        {type === "card" && (
          <>
            {field(t("pw.field_cardholder"), "card_holder", t("pw.placeholder_cardholder"))}
            {field(t("pw.field_number"), "card_number", "1234 5678 9012 3456", true)}
            {field(t("pw.field_expiry_month"), "card_exp_month", t("pw.placeholder_month"))}
            {field(t("pw.field_expiry_year"), "card_exp_year", t("pw.placeholder_year"))}
            {field(t("pw.field_security_code"), "card_code", "CVC")}
            {field(t("pw.field_brand"), "card_brand", t("pw.placeholder_brand"))}
          </>
        )}

        {type === "identity" && (
          <>
            {field(t("pw.field_full_name"), "id_full_name", t("pw.placeholder_full_name"))}
            {field(t("pw.field_company"), "id_company")}
            {field(t("pw.field_email"), "id_email")}
            {field(t("pw.field_phone"), "id_phone")}
            {field(t("pw.field_address"), "id_address", t("pw.placeholder_address"), true)}
            {field(t("pw.field_city"), "id_city")}
            {field(t("pw.field_state"), "id_state")}
            {field(t("pw.field_postal_code"), "id_zip")}
            {field(t("pw.field_country"), "id_country")}
          </>
        )}

        {type === "ssh_key" && (
          <>
            <div className="field field-full">
              <span>{t("pw.field_key_pair")}</span>
              {(draft.ssh_private_key ?? "").trim() === "" ? (
                <div className="pw-attachments">
                  <button
                    type="button"
                    className="pw-attach-btn"
                    disabled={sshBusy}
                    onClick={() => void generateSshKey()}
                  >
                    <IconGenerate size={14} />
                    <span>{sshBusy ? t("pw.ssh_generating") : t("pw.ssh_generate")}</span>
                  </button>
                </div>
              ) : (
                <p className="hint">{t("pw.ssh_clear_to_generate")}</p>
              )}
              {sshError && <p className="hint is-error">{sshError}</p>}
            </div>
            <label className="field field-full">
              <span>{t("pw.field_private_key")}</span>
              <textarea
                rows={5}
                autoComplete="off"
                placeholder="-----BEGIN OPENSSH PRIVATE KEY-----"
                spellCheck={false}
                value={draft.ssh_private_key ?? ""}
                onChange={(e) => setDraft({ ...draft, ssh_private_key: e.target.value })}
              />
            </label>
            <label className="field field-full">
              <span>{t("pw.field_public_key")}</span>
              <textarea
                rows={2}
                autoComplete="off"
                placeholder="ssh-ed25519 …"
                spellCheck={false}
                value={draft.ssh_public_key ?? ""}
                onChange={(e) => setDraft({ ...draft, ssh_public_key: e.target.value })}
              />
            </label>
            {field(t("pw.field_fingerprint"), "ssh_fingerprint", "SHA256:…", true)}
            <SshAgentOption
              draft={draft}
              onChange={(changes) => setDraft((d) => ({ ...d, ...changes }))}
            />
          </>
        )}

        {type === "login" && (
          <>
        <label className="field">
          <span>{t("pw.field_username_or_email")}</span>
          <input
            type="text"
            autoComplete="off"
            placeholder="your@email.com"
            value={draft.username}
            onChange={(e) => setDraft({ ...draft, username: e.target.value })}
          />
        </label>
        <label className="field field-full">
          <span>{t("pw.field_password")}</span>
          <div className="pw-password-input-row">
            <input
              type={passwordVisible ? "text" : "password"}
              autoComplete="off"
              value={draft.password}
              onChange={(e) => setDraft({ ...draft, password: e.target.value })}
            />
            <button
              type="button"
              className="pw-gen-btn"
              data-tooltip={passwordVisible ? t("pw.hide") : t("pw.show")}
              aria-label={passwordVisible ? t("pw.hide") : t("pw.show")}
              onClick={() => setPasswordVisible((v) => !v)}
            >
              {passwordVisible ? <IconEyeOff size={15} /> : <IconEye size={15} />}
            </button>
            <button
              type="button"
              className="pw-gen-btn"
              data-tooltip={t("pw.copy_password")}
              aria-label={t("pw.copy_password")}
              onClick={() => void copySecret(draft.password, "password")}
            >
              <IconCopy size={15} />
            </button>
            <button
              type="button"
              className="pw-gen-btn accent"
              data-tooltip={t("pw.generate_password")}
              aria-label={t("pw.generate_password")}
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
              {t("pw.generator_options")}
            </button>
          </div>

          {genOpen && (
            <div className="pw-gen-panel">
              <div className="pw-gen-panel-header">
                <span>{t("pw.generator")}</span>
                <span className="pw-gen-length-value">
                  {t("pw.generator_length", { count: genOptions.length })}
                </span>
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
          <span>{t("pw.field_totp")}</span>
          <div className="pw-totp-panel">
            {draft.totp_secret && !totpError && !totpTyping ? (
              <>
                <TotpDisplay
                  entry={draft}
                  now={now}
                  copied={false}
                  onCopy={(code) => void copySecret(code, "one-time code")}
                />
                <button type="button" className="pw-totp-remove-btn" onClick={() => applyTotpInput("")}>
                  <IconClose size={13} />
                  <span>{t("pw.remove")}</span>
                </button>
              </>
            ) : (
              <>
                <input
                  type="text"
                  placeholder={t("pw.totp_placeholder")}
                  value={totpInput}
                  onChange={(e) => applyTotpInput(e.target.value)}
                  onFocus={() => setTotpTyping(true)}
                  onBlur={() => setTotpTyping(false)}
                  aria-invalid={totpError ? true : undefined}
                  autoComplete="off"
                  spellCheck={false}
                />
                <p className={`hint pw-totp-hint${totpError ? " pw-totp-hint-error" : ""}`}>
                  {totpError ? t("pw.totp_invalid") : t("pw.totp_hint")}
                </p>
              </>
            )}
          </div>
        </label>

        <label className="field">
          <span>{t("pw.field_website")}</span>
          <input
            type="url"
            autoComplete="off"
            placeholder="https://example.com"
            value={draft.url}
            onChange={(e) => setDraft({ ...draft, url: e.target.value })}
          />
        </label>
          </>
        )}

        <label className="field">
          <span>{t("pw.field_category")}</span>
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
          <span>{t("pw.field_custom_fields")}</span>
          <div className="pw-custom-fields">
            {(draft.fields ?? []).map((field, i) => {
              const update = (change: Partial<CustomField>) =>
                setDraft((d) => ({
                  ...d,
                  fields: (d.fields ?? []).map((f, j) => (j === i ? { ...f, ...change } : f)),
                }));
              return (
                <div key={i} className="pw-custom-field">
                  <input
                    type="text"
                    aria-label={t("pw.custom_name_label")}
                    placeholder={t("pw.field_name")}
                    autoComplete="off"
                    value={field.name}
                    onChange={(e) => update({ name: e.target.value })}
                  />
                  <input
                    type={field.hidden && !passwordVisible ? "password" : "text"}
                    aria-label={
                      field.name
                        ? t("pw.custom_value_label", { name: field.name })
                        : t("pw.custom_value_label_unnamed")
                    }
                    placeholder={t("pw.custom_value")}
                    autoComplete="off"
                    spellCheck={false}
                    value={field.value}
                    onChange={(e) => update({ value: e.target.value })}
                  />
                  <label className="pw-custom-hidden" data-tooltip={t("pw.custom_hidden_tip")}>
                    <input
                      type="checkbox"
                      checked={field.hidden}
                      onChange={(e) => update({ hidden: e.target.checked })}
                    />
                    <span>{t("pw.custom_hidden")}</span>
                  </label>
                  <button
                    type="button"
                    className="pw-inline-btn danger"
                    data-tooltip={t("pw.custom_remove")}
                    aria-label={
                      field.name
                        ? t("pw.custom_remove_label", { name: field.name })
                        : t("pw.custom_remove_label_unnamed")
                    }
                    onClick={() =>
                      setDraft((d) => ({
                        ...d,
                        fields: (d.fields ?? []).filter((_, j) => j !== i),
                      }))
                    }
                  >
                    <IconTrash size={13} />
                  </button>
                </div>
              );
            })}
            <button
              type="button"
              className="pw-attach-btn"
              onClick={() =>
                setDraft((d) => ({
                  ...d,
                  fields: [...(d.fields ?? []), { name: "", value: "", hidden: false }],
                }))
              }
            >
              <IconPlus size={14} />
              <span>{t("pw.custom_add")}</span>
            </button>
          </div>
          <p className="hint">{t("pw.custom_hint")}</p>
        </div>

        <div className="field field-full">
          <span>{t("pw.field_attached_files")}</span>
          <div className="pw-attachments">
            {(draft.attachments ?? []).map((a) => (
              <div key={a.blob_id} className="pw-attachment-row">
                <Paperclip size={14} aria-hidden />
                <span className="pw-attachment-name">{a.name}</span>
                <span className="pw-attachment-size">{formatBytes(a.size_bytes)}</span>
                <button
                  type="button"
                  className="pw-inline-btn danger"
                  data-tooltip={t("pw.attach_remove")}
                  aria-label={t("pw.attach_remove")}
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
              <span>{attachBusy ? t("pw.attach_encrypting") : t("pw.attach_add")}</span>
            </button>
          </div>
          {attachError && <p className="hint is-error">{attachError}</p>}
          <p className="hint">{t("pw.attach_hint")}</p>
        </div>

        <div className="field field-full">
          <span>{t("pw.field_protection")}</span>
          <label className="pw-reauth-toggle">
            <input
              type="checkbox"
              checked={draft.require_reauth ?? false}
              onChange={(e) =>
                setDraft({ ...draft, require_reauth: e.target.checked || undefined })
              }
            />
            <span>
              {platformStrings(os).hasBuiltIn
                ? t("pw.reauth_option_builtin", { builtIn: platformStrings(os).builtIn })
                : t("pw.reauth_option")}
            </span>
          </label>
          <p className="hint">{t("pw.reauth_hint")}</p>
        </div>

        <label className="field field-full">
          <span>{t("pw.field_notes")}</span>
          <textarea
            rows={type === "note" ? 10 : 6}
            autoComplete="off"
            placeholder={
              type === "note" ? t("pw.placeholder_note") : t("pw.placeholder_notes")
            }
            value={draft.notes}
            onChange={(e) => setDraft({ ...draft, notes: e.target.value })}
          />
        </label>
      </div>
      <div className="pw-editor-actions">
        {!canSave && (
          <span className="hint">{stillNeeded}</span>
        )}
        <button type="button" className="btn-secondary" onClick={handleCancel}>
          {t("common.cancel")}
        </button>
        <button
          className="btn-primary"
          type="button"
          disabled={!canSave}
          data-tooltip={!canSave ? stillNeeded : undefined}
          onClick={handleSave}
        >
          {creating ? t("pw.add") : t("pw.save")}
        </button>
      </div>
    </div>
  );
}
