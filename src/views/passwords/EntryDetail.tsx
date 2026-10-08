import { Fragment, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Contact,
  CreditCard,
  History,
  Paperclip,
  Star,
  StickyNote,
  TerminalSquare,
} from "lucide-react";
import type { HistoryVersion, PasswordAttachment, PasswordEntry } from "../../lib/types";
import { formatBytes, formatDate } from "../../lib/format";
import { changedLabels } from "../../lib/entryHistory";
import { TotpDisplay } from "./TotpDisplay";
import {
  avatarColor,
  cardDigits,
  faviconUrl,
  groupCardNumber,
  inkOn,
  normalizeUrl,
  notesAreSecret,
  serviceInitials,
  typeOf,
} from "./util";
import { IconCopy, IconEdit, IconEye, IconEyeOff, IconTrash } from "../../ui/Icons";
import { t, useLocale } from "../../i18n";

type Props = {
  entry: PasswordEntry;
  now: number;
  showFavicons: boolean;
  copiedId: string | null;
  colorFor: (category: string) => string;
  busy: boolean;
  onCopyUsername: (entry: PasswordEntry) => void;
  onCopyTotp: (entry: PasswordEntry, code: string) => void;
  /** Non-secret text: goes to the ordinary clipboard. */
  onCopyPlain: (key: string, text: string) => void;
  /** A secret of this entry: re-auth gate, then the clearing clipboard. */
  /// `field` names what was copied, for the silo's activity log.
  onCopySecretField: (entry: PasswordEntry, key: string, text: string, field: string) => void;
  onOpenAttachment: (attachment: PasswordAttachment) => void;
  /** Resolves true when this entry may be shown: either it is unprotected,
   * or the user just proved presence with an enrolled authenticator. */
  onRequestReveal: (entry: PasswordEntry) => Promise<boolean>;
  onEdit: (entry: PasswordEntry) => void;
  onDelete: (id: string) => void;
  onToggleFavorite: (entry: PasswordEntry) => void;
  /** Saves `version` as the entry's current state; the panel asks first. */
  onRestoreVersion: (entry: PasswordEntry, version: HistoryVersion) => void;
  onClearHistory: (entry: PasswordEntry) => void;
};

/**
 * One entry, read-only, filling the detail pane.
 *
 * Reveal state lives here and is tied to the entry it was granted for, so
 * moving to another entry or another view never leaves a previously
 * revealed password on screen.
 */
export function EntryDetail({
  entry,
  now,
  showFavicons,
  copiedId,
  colorFor,
  busy,
  onCopyUsername,
  onCopyTotp,
  onCopyPlain,
  onCopySecretField,
  onOpenAttachment,
  onRequestReveal,
  onEdit,
  onDelete,
  onToggleFavorite,
  onRestoreVersion,
  onClearHistory,
}: Props) {
  useLocale();
  /// Which entry the user asked to see, rather than a bare "revealed" flag.
  ///
  /// Derived, so a reveal cannot outlive the entry it was granted for. The
  /// flag version did: this pane is one component with the selected entry as
  /// a prop, so clicking the next row swapped the entry and kept the
  /// `true`, and the next password, card number and security code were on
  /// screen without anyone asking for them, including for an entry that
  /// wants a key touch first. The parent also keys this component on the
  /// entry, which fixes it too; this is the half that cannot be dropped by
  /// accident later.
  const [shownFor, setShownFor] = useState<string | null>(null);
  const revealed = shownFor === entry.id;
  const [faviconFailed, setFaviconFailed] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const history = entry.history ?? [];

  const type = typeOf(entry);
  const icon = type === "login" && showFavicons && entry.url ? faviconUrl(entry.url) : null;
  const showIcon = icon !== null && !faviconFailed;

  /// One reveal per entry, covering whichever secrets its kind has: asking
  /// separately for a card's number and its code would be two touches for
  /// what is one act of reading the card.
  const toggleReveal = () => {
    if (revealed) {
      setShownFor(null);
      return;
    }
    void onRequestReveal(entry).then((allowed) => {
      if (allowed) setShownFor(entry.id);
    });
  };

  const revealButton = (
    <button
      type="button"
      className="pw-inline-btn"
      title={revealed ? t("pw.hide") : t("pw.show")}
      aria-label={revealed ? t("pw.hide") : t("pw.show")}
      onClick={toggleReveal}
    >
      {revealed ? <IconEyeOff size={14} /> : <IconEye size={14} />}
    </button>
  );

  const copyBadge = (key: string, icon_: React.ReactNode = <IconCopy size={14} />) =>
    copiedId === key ? <span className="pw-copied-badge">{t("pw.copied")}</span> : icon_;

  /// `copyLabel` names the copy button; `audit`, below, names the field for
  /// the activity log, which stays in English.
  const plainRow = (
    label: string,
    value: string | undefined,
    copyKey?: string,
    copyLabel?: string,
  ) =>
    value ? (
      <div className="pw-field-row">
        <span className="pw-field-label">{label}</span>
        <span className="pw-field-value">{value}</span>
        {copyKey && (
          <button
            type="button"
            className="pw-inline-btn"
            title={copyLabel}
            aria-label={copyLabel}
            onClick={() => onCopyPlain(copyKey, value)}
          >
            {copyBadge(copyKey)}
          </button>
        )}
      </div>
    ) : null;

  const secretRow = (
    label: string,
    value: string,
    shown: string,
    copyKey: string,
    copyLabel: string,
    audit: string,
  ) => (
    <div className="pw-field-row">
      <span className="pw-field-label">{label}</span>
      <span className="pw-field-value pw-mask">{revealed ? shown : "••••••••••••"}</span>
      {revealButton}
      <button
        type="button"
        className="pw-inline-btn"
        title={copyLabel}
        aria-label={copyLabel}
        onClick={() => onCopySecretField(entry, copyKey, value, audit)}
      >
        {copyBadge(copyKey)}
      </button>
    </div>
  );

  return (
    <div className="pw-detail">
      <div className="pw-detail-head">
        {/* The tint is for initials. Over a real site icon it repainted the
            logo in the category colour, which made every icon look wrong. */}
        <div
          className={`pw-card-avatar${showIcon ? " has-favicon" : ""}`}
          style={
            showIcon
              ? undefined
              : {
                  background: avatarColor(colorFor(entry.category)),
                  color: inkOn(avatarColor(colorFor(entry.category))),
                }
          }
        >
          {showIcon ? (
            <img src={icon} alt="" className="pw-card-favicon" onError={() => setFaviconFailed(true)} />
          ) : type === "card" ? (
            <CreditCard size={20} aria-hidden />
          ) : type === "identity" ? (
            <Contact size={20} aria-hidden />
          ) : type === "ssh_key" ? (
            <TerminalSquare size={20} aria-hidden />
          ) : type === "note" ? (
            <StickyNote size={20} aria-hidden />
          ) : (
            serviceInitials(entry.service || "??")
          )}
        </div>
        <div className="pw-card-title">
          <span className="pw-detail-service">{entry.service || t("pw.untitled")}</span>
          <span className="pw-card-category">
            <span className="pw-rail-dot" style={{ background: colorFor(entry.category) }} aria-hidden />
            {entry.category}
          </span>
        </div>
        <div className="pw-card-actions">
          <button
            type="button"
            className={`pw-action-btn${entry.favorite ? " is-starred" : ""}`}
            title={entry.favorite ? t("pw.unfavorite") : t("pw.favorite")}
            aria-pressed={entry.favorite ?? false}
            disabled={busy}
            onClick={() => onToggleFavorite(entry)}
          >
            <Star size={15} fill={entry.favorite ? "currentColor" : "none"} />
          </button>
          <button
            type="button"
            className="pw-action-btn"
            title={t("pw.edit")}
            disabled={busy}
            onClick={() => onEdit(entry)}
          >
            <IconEdit size={15} />
          </button>
          {/* The confirmation is a modal, asked by the panel. It used to be
              two small buttons that replaced the trash icon in place, which
              put Confirm exactly where the cursor already was and said
              nothing about which entry was about to go. */}
          <button
            type="button"
            className="pw-action-btn danger"
            title={t("pw.delete")}
            disabled={busy}
            onClick={() => onDelete(entry.id)}
          >
            <IconTrash size={15} />
          </button>
        </div>
      </div>

      {/* The rows themselves are not copy targets. Making the whole row
          clickable meant any stray click put a password on the clipboard,
          with a 12px badge as the only sign it had happened. Copying is what
          the copy button is for. */}
      <div className="pw-card-fields">
        {type === "login" && (
          <>
            <div className="pw-field-row">
              <span className="pw-field-label">{t("pw.field_username_or_email")}</span>
              <span className="pw-field-value">{entry.username || "-"}</span>
              <button
                type="button"
                className="pw-inline-btn"
                title={t("pw.copy_username")}
                aria-label={t("pw.copy_username")}
                onClick={() => onCopyUsername(entry)}
              >
                {copyBadge(`u-${entry.id}`)}
              </button>
            </div>
            {secretRow(
              t("pw.field_password"),
              entry.password,
              entry.password,
              entry.id,
              t("pw.copy_password"),
              "password",
            )}
            {entry.totp_secret && (
              <TotpDisplay
                entry={entry}
                now={now}
                hidden={entry.require_reauth === true && !revealed}
                copied={copiedId === `t-${entry.id}`}
                onCopy={(code) => onCopyTotp(entry, code)}
              />
            )}
          </>
        )}

        {type === "card" && (
          <>
            {plainRow(
              t("pw.field_cardholder"),
              entry.card_holder,
              `h-${entry.id}`,
              t("pw.copy_cardholder"),
            )}
            {plainRow(t("pw.field_brand"), entry.card_brand)}
            {secretRow(
              t("pw.field_number"),
              cardDigits(entry),
              groupCardNumber(cardDigits(entry)),
              entry.id,
              t("pw.copy_number"),
              "number",
            )}
            {(entry.card_exp_month || entry.card_exp_year) && (
              <div className="pw-field-row">
                <span className="pw-field-label">{t("pw.field_expiry")}</span>
                <span className="pw-field-value">
                  {entry.card_exp_month || "??"}/{entry.card_exp_year || "??"}
                </span>
              </div>
            )}
            {entry.card_code &&
              secretRow(
                t("pw.field_security_code"),
                entry.card_code,
                entry.card_code,
                `c-${entry.id}`,
                t("pw.copy_security_code"),
                "security code",
              )}
          </>
        )}

        {type === "identity" && (
          <>
            {plainRow(
              t("pw.field_person_name"),
              entry.id_full_name,
              `n-${entry.id}`,
              t("pw.copy_person_name"),
            )}
            {plainRow(t("pw.field_company"), entry.id_company)}
            {plainRow(t("pw.field_email"), entry.id_email, `e-${entry.id}`, t("pw.copy_email"))}
            {plainRow(t("pw.field_phone"), entry.id_phone, `p-${entry.id}`, t("pw.copy_phone"))}
            {plainRow(
              t("pw.field_address"),
              [entry.id_address, entry.id_city, entry.id_state, entry.id_zip, entry.id_country]
                .filter(Boolean)
                .join(", "),
              `a-${entry.id}`,
              t("pw.copy_address"),
            )}
          </>
        )}

        {type === "ssh_key" && (
          <>
            {plainRow(
              t("pw.field_fingerprint"),
              entry.ssh_fingerprint,
              `f-${entry.id}`,
              t("pw.copy_fingerprint"),
            )}
            {entry.ssh_public_key && (
              <div className="pw-field-row">
                <span className="pw-field-label">{t("pw.field_public_key")}</span>
                <span className="pw-field-value pw-pubkey">{entry.ssh_public_key}</span>
                <button
                  type="button"
                  className="pw-inline-btn"
                  title={t("pw.copy_public_key")}
                  aria-label={t("pw.copy_public_key")}
                  onClick={() => onCopyPlain(`k-${entry.id}`, entry.ssh_public_key ?? "")}
                >
                  {copyBadge(`k-${entry.id}`)}
                </button>
              </div>
            )}
            {/* No reveal for a private key: a multi-line PEM block cannot
                usefully show in a row, and everything that needs it (an
                ssh config, an agent) takes a paste. Copy is gated the same
                as a password. */}
            <div className="pw-field-row">
              <span className="pw-field-label">{t("pw.field_private_key")}</span>
              <span className="pw-field-value pw-mask">••••••••••••</span>
              <button
                type="button"
                className="pw-inline-btn"
                title={t("pw.copy_private_key")}
                aria-label={t("pw.copy_private_key")}
                onClick={() =>
                  onCopySecretField(entry, entry.id, entry.ssh_private_key ?? "", "private key")
                }
              >
                {copyBadge(entry.id)}
              </button>
            </div>
          </>
        )}

        {/* After the kind's own fields: they are this entry's, named by the
            user. A hidden one is covered by the same reveal as the password. */}
        {(entry.fields ?? []).map((field, i) => (
          <Fragment key={`f${i}`}>
            {field.hidden
              ? secretRow(
                  field.name || t("pw.hidden_field"),
                  field.value,
                  field.value,
                  `f${i}-${entry.id}`,
                  field.name
                    ? t("pw.copy_named", { name: field.name.toLowerCase() })
                    : t("pw.copy_hidden_field"),
                  (field.name || "Hidden field").toLowerCase(),
                )
              : plainRow(
                  field.name || t("pw.field"),
                  field.value,
                  `f${i}-${entry.id}`,
                  field.name
                    ? t("pw.copy_named", { name: field.name.toLowerCase() })
                    : t("pw.copy_field"),
                )}
          </Fragment>
        ))}

        {entry.url &&
          (() => {
            // Not a website: shown as it was saved, but not turned into
            // something clickable. See normalizeUrl.
            const href = normalizeUrl(entry.url);
            if (!href) {
              return (
                <div className="pw-field-row">
                  <span className="pw-field-label">{t("pw.field_website")}</span>
                  <span className="pw-field-value">{entry.url}</span>
                </div>
              );
            }
            return (
              <div
                className="pw-field-row is-clickable"
                role="link"
                tabIndex={0}
                onClick={() => void openUrl(href)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    void openUrl(href);
                  }
                }}
              >
                <span className="pw-field-label">{t("pw.field_website")}</span>
                <span className="pw-field-value pw-link">{entry.url}</span>
              </div>
            );
          })()}

        {(entry.attachments ?? []).length > 0 && (
          <div className="pw-field-row pw-field-notes">
            <span className="pw-field-label">{t("pw.field_attached_files")}</span>
            <div className="pw-attachments">
              {entry.attachments!.map((a) => (
                <button
                  key={a.blob_id}
                  type="button"
                  className="pw-attachment-row is-clickable"
                  title={t("pw.open_attachment")}
                  onClick={() => onOpenAttachment(a)}
                >
                  <Paperclip size={14} aria-hidden />
                  <span className="pw-attachment-name">{a.name}</span>
                  <span className="pw-attachment-size">{formatBytes(a.size_bytes)}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Notes follow the entry's own answer. On an ordinary entry they
            are context and show as they always did; on one the user ticked
            "ask again before revealing" they are covered by the same single
            reveal as the password, because a note beside a protected
            password is usually where the recovery codes went. A note-type
            entry is nothing but its note, so the rule matters most there. */}
        {entry.notes && (
          <div className="pw-field-row pw-field-notes">
            <span className="pw-field-label">{t("pw.field_notes")}</span>
            <span className="pw-field-value pw-notes-value">
              {notesAreSecret(entry) && !revealed
                ? "••••••••••••"
                : entry.notes}
            </span>
            {notesAreSecret(entry) && revealButton}
            <button
              type="button"
              className="pw-inline-btn"
              title={t("pw.copy_notes")}
              aria-label={t("pw.copy_notes")}
              onClick={() => onCopySecretField(entry, `notes-${entry.id}`, entry.notes, "notes")}
            >
              {copyBadge(`notes-${entry.id}`)}
            </button>
          </div>
        )}

        {/* Closed until asked for: old passwords are secrets too, and the
            list is about recovering something, not reading it every time. */}
        {history.length > 0 && (
          <div className="pw-field-row pw-field-notes">
            <span className="pw-field-label">{t("pw.history_label")}</span>
            <div className="pw-history">
              <button
                type="button"
                className="pw-history-toggle"
                aria-expanded={historyOpen}
                onClick={() => setHistoryOpen((open) => !open)}
              >
                <History size={14} aria-hidden />
                {t("pw.history_versions", { count: history.length })}
              </button>
              {historyOpen && (
                <>
                  {history.map((version, i) => {
                    const newer = i === 0 ? entry : history[i - 1];
                    const changed = changedLabels(version, newer);
                    return (
                      <div key={`${version.saved_at}-${i}`} className="pw-history-row">
                        <span className="pw-history-date">{formatDate(version.saved_at)}</span>
                        <span className="pw-history-what">
                          {changed.length > 0
                            ? t("pw.history_next_change", { changes: changed.join(", ") })
                            : t("pw.history_no_change")}
                        </span>
                        {version.password && (
                          <>
                            <span className="pw-field-value pw-mask">
                              {revealed ? version.password : "••••••••"}
                            </span>
                            <button
                              type="button"
                              className="pw-inline-btn"
                              title={t("pw.copy_this_password")}
                              aria-label={t("pw.copy_this_password")}
                              onClick={() =>
                                onCopySecretField(
                                  entry,
                                  `h${i}-${entry.id}`,
                                  version.password ?? "",
                                  "earlier password",
                                )
                              }
                            >
                              {copyBadge(`h${i}-${entry.id}`)}
                            </button>
                          </>
                        )}
                        <button
                          type="button"
                          className="btn-secondary btn-sm pw-history-restore"
                          disabled={busy}
                          aria-label={t("pw.history_restore_label", {
                            date: formatDate(version.saved_at),
                          })}
                          onClick={() => onRestoreVersion(entry, version)}
                        >
                          {t("pw.history_restore")}
                        </button>
                      </div>
                    );
                  })}
                  <button
                    type="button"
                    className="link danger pw-history-clear"
                    disabled={busy}
                    onClick={() => onClearHistory(entry)}
                  >
                    {t("pw.clear_history")}
                  </button>
                </>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
