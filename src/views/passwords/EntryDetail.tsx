import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Contact, CreditCard, Paperclip, Star, StickyNote, TerminalSquare } from "lucide-react";
import type { PasswordAttachment, PasswordEntry } from "../../lib/types";
import { formatBytes } from "../../lib/format";
import { TotpDisplay } from "./TotpDisplay";
import {
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
  onCopySecretField: (entry: PasswordEntry, key: string, text: string) => void;
  onOpenAttachment: (attachment: PasswordAttachment) => void;
  /** Resolves true when this entry may be shown: either it is unprotected,
   * or the user just proved presence with an enrolled authenticator. */
  onRequestReveal: (entry: PasswordEntry) => Promise<boolean>;
  onEdit: (entry: PasswordEntry) => void;
  onDelete: (id: string) => void;
  onToggleFavorite: (entry: PasswordEntry) => void;
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
}: Props) {
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
      title={revealed ? "Hide" : "Reveal"}
      aria-label={revealed ? "Hide" : "Reveal"}
      onClick={toggleReveal}
    >
      {revealed ? <IconEyeOff size={14} /> : <IconEye size={14} />}
    </button>
  );

  const copyBadge = (key: string, icon_: React.ReactNode = <IconCopy size={14} />) =>
    copiedId === key ? <span className="pw-copied-badge">Copied</span> : icon_;

  const plainRow = (label: string, value: string | undefined, copyKey?: string) =>
    value ? (
      <div className="pw-field-row">
        <span className="pw-field-label">{label}</span>
        <span className="pw-field-value">{value}</span>
        {copyKey && (
          <button
            type="button"
            className="pw-inline-btn"
            title={`Copy ${label.toLowerCase()}`}
            aria-label={`Copy ${label.toLowerCase()}`}
            onClick={() => onCopyPlain(copyKey, value)}
          >
            {copyBadge(copyKey)}
          </button>
        )}
      </div>
    ) : null;

  const secretRow = (label: string, value: string, shown: string, copyKey: string) => (
    <div className="pw-field-row">
      <span className="pw-field-label">{label}</span>
      <span className="pw-field-value pw-mask">{revealed ? shown : "••••••••••••"}</span>
      {revealButton}
      <button
        type="button"
        className="pw-inline-btn"
        title={`Copy ${label.toLowerCase()}`}
        aria-label={`Copy ${label.toLowerCase()}`}
        onClick={() => onCopySecretField(entry, copyKey, value)}
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
                  background: colorFor(entry.category),
                  color: inkOn(colorFor(entry.category)),
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
          <span className="pw-detail-service">{entry.service || "Untitled"}</span>
          <span className="pw-card-category" style={{ color: colorFor(entry.category) }}>
            {entry.category}
          </span>
        </div>
        <div className="pw-card-actions">
          <button
            type="button"
            className={`pw-action-btn${entry.favorite ? " is-starred" : ""}`}
            title={entry.favorite ? "Remove from favourites" : "Add to favourites"}
            aria-pressed={entry.favorite ?? false}
            disabled={busy}
            onClick={() => onToggleFavorite(entry)}
          >
            <Star size={15} fill={entry.favorite ? "currentColor" : "none"} />
          </button>
          <button
            type="button"
            className="pw-action-btn"
            title="Edit"
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
            title="Delete"
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
              <span className="pw-field-label">Username</span>
              <span className="pw-field-value">{entry.username || "—"}</span>
              <button
                type="button"
                className="pw-inline-btn"
                title="Copy username"
                aria-label="Copy username"
                onClick={() => onCopyUsername(entry)}
              >
                {copyBadge(`u-${entry.id}`)}
              </button>
            </div>
            {secretRow("Password", entry.password, entry.password, entry.id)}
            {entry.totp_secret && (
              <TotpDisplay
                entry={entry}
                now={now}
                copied={copiedId === `t-${entry.id}`}
                onCopy={(code) => onCopyTotp(entry, code)}
              />
            )}
          </>
        )}

        {type === "card" && (
          <>
            {plainRow("Cardholder", entry.card_holder, `h-${entry.id}`)}
            {plainRow("Brand", entry.card_brand)}
            {secretRow(
              "Number",
              cardDigits(entry),
              groupCardNumber(cardDigits(entry)),
              entry.id
            )}
            {(entry.card_exp_month || entry.card_exp_year) && (
              <div className="pw-field-row">
                <span className="pw-field-label">Expires</span>
                <span className="pw-field-value">
                  {entry.card_exp_month || "??"}/{entry.card_exp_year || "??"}
                </span>
              </div>
            )}
            {entry.card_code &&
              secretRow("Code", entry.card_code, entry.card_code, `c-${entry.id}`)}
          </>
        )}

        {type === "identity" && (
          <>
            {plainRow("Name", entry.id_full_name, `n-${entry.id}`)}
            {plainRow("Company", entry.id_company)}
            {plainRow("Email", entry.id_email, `e-${entry.id}`)}
            {plainRow("Phone", entry.id_phone, `p-${entry.id}`)}
            {plainRow(
              "Address",
              [entry.id_address, entry.id_city, entry.id_state, entry.id_zip, entry.id_country]
                .filter(Boolean)
                .join(", "),
              `a-${entry.id}`
            )}
          </>
        )}

        {type === "ssh_key" && (
          <>
            {plainRow("Fingerprint", entry.ssh_fingerprint, `f-${entry.id}`)}
            {entry.ssh_public_key && (
              <div className="pw-field-row">
                <span className="pw-field-label">Public key</span>
                <span className="pw-field-value pw-pubkey">{entry.ssh_public_key}</span>
                <button
                  type="button"
                  className="pw-inline-btn"
                  title="Copy public key"
                  aria-label="Copy public key"
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
              <span className="pw-field-label">Private key</span>
              <span className="pw-field-value pw-mask">••••••••••••</span>
              <button
                type="button"
                className="pw-inline-btn"
                title="Copy private key"
                aria-label="Copy private key"
                onClick={() =>
                  onCopySecretField(entry, entry.id, entry.ssh_private_key ?? "")
                }
              >
                {copyBadge(entry.id)}
              </button>
            </div>
          </>
        )}

        {entry.url &&
          (() => {
            // Not a website: shown as it was saved, but not turned into
            // something clickable. See normalizeUrl.
            const href = normalizeUrl(entry.url);
            if (!href) {
              return (
                <div className="pw-field-row">
                  <span className="pw-field-label">Website</span>
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
                <span className="pw-field-label">Website</span>
                <span className="pw-field-value pw-link">{entry.url}</span>
              </div>
            );
          })()}

        {(entry.attachments ?? []).length > 0 && (
          <div className="pw-field-row pw-field-notes">
            <span className="pw-field-label">Files</span>
            <div className="pw-attachments">
              {entry.attachments!.map((a) => (
                <button
                  key={a.blob_id}
                  type="button"
                  className="pw-attachment-row is-clickable"
                  title="Open (fetched from backup if not on this computer)"
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
            <span className="pw-field-label">Notes</span>
            <span className="pw-field-value pw-notes-value">
              {notesAreSecret(entry) && !revealed
                ? "••••••••••••"
                : entry.notes}
            </span>
            {notesAreSecret(entry) && revealButton}
            <button
              type="button"
              className="pw-inline-btn"
              title="Copy notes"
              aria-label="Copy notes"
              onClick={() => onCopySecretField(entry, `notes-${entry.id}`, entry.notes)}
            >
              {copyBadge(`notes-${entry.id}`)}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
