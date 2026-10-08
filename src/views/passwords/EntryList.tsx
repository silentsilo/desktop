import { useState } from "react";
import { Contact, CreditCard, StickyNote, TerminalSquare, User } from "lucide-react";
import type { PasswordEntry } from "../../lib/types";
import { avatarColor, faviconUrl, inkOn, serviceInitials, subtitleFor, typeOf } from "./util";
import { IconCopy } from "../../ui/Icons";
import { t, useLocale } from "../../i18n";

type Props = {
  entries: PasswordEntry[];
  selectedId: string | null;
  showFavicons: boolean;
  copiedId: string | null;
  colorFor: (category: string) => string;
  onSelect: (id: string) => void;
  onCopyUsername: (entry: PasswordEntry) => void;
  onCopyPassword: (entry: PasswordEntry) => void;
};

/**
 * The middle pane: every entry as a compact row.
 *
 * Rows, not cards. A card per login spent three lines on fields the user
 * mostly does not need until they have picked an entry, which capped the
 * screen at four or five logins. A row shows enough to choose — icon, name,
 * username — and everything else belongs to the detail pane.
 */
export function EntryList({
  entries,
  selectedId,
  showFavicons,
  copiedId,
  colorFor,
  onSelect,
  onCopyUsername,
  onCopyPassword,
}: Props) {
  useLocale();
  const [faviconErrorIds, setFaviconErrorIds] = useState<Set<string>>(new Set());

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    if (entries.length === 0) return;
    const index = entries.findIndex((entry) => entry.id === selectedId);
    const next =
      e.key === "ArrowDown"
        ? Math.min(index + 1, entries.length - 1)
        : Math.max(index - 1, 0);
    onSelect(entries[next]!.id);
  };

  return (
    // Listbox semantics: one focus stop, arrows move the selection — the
    // same contract the file explorer's list keeps.
    <div
      className="pw-rows"
      role="listbox"
      aria-label={t("nav.passwords")}
      tabIndex={0}
      onKeyDown={onKeyDown}
    >
      {entries.map((entry) => {
        const type = typeOf(entry);
        const icon =
          type === "login" && showFavicons && entry.url ? faviconUrl(entry.url) : null;
        const showIcon = icon !== null && !faviconErrorIds.has(entry.id);
        return (
          <div
            key={entry.id}
            role="option"
            aria-selected={entry.id === selectedId}
            className={`pw-row${entry.id === selectedId ? " is-selected" : ""}`}
            onClick={() => onSelect(entry.id)}
          >
            <div
              className={`pw-row-avatar${showIcon ? " has-favicon" : ""}`}
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
                <img
                  src={icon}
                  alt=""
                  className="pw-card-favicon"
                  onError={() => setFaviconErrorIds((prev) => new Set(prev).add(entry.id))}
                />
              ) : type === "card" ? (
                <CreditCard size={15} aria-hidden />
              ) : type === "identity" ? (
                <Contact size={15} aria-hidden />
              ) : type === "ssh_key" ? (
                <TerminalSquare size={15} aria-hidden />
              ) : type === "note" ? (
                <StickyNote size={15} aria-hidden />
              ) : (
                serviceInitials(entry.service || "??")
              )}
            </div>
            <div className="pw-row-text">
              <span className="pw-row-service">{entry.service || t("pw.untitled")}</span>
              <span className="pw-row-username">{subtitleFor(entry) || " "}</span>
            </div>
            {/* Quick copies without opening the entry — the two things done
                far more often than everything else put together. Stopping
                propagation so a copy is not also a selection change. */}
            <div className="pw-row-actions">
              {type === "login" && (
                <button
                  type="button"
                  className="pw-inline-btn"
                  title={t("pw.copy_username")}
                  aria-label={t("pw.list_copy_username_label", {
                    name: entry.service || t("pw.untitled_entry"),
                  })}
                  onClick={(e) => {
                    e.stopPropagation();
                    onCopyUsername(entry);
                  }}
                >
                  {copiedId === `u-${entry.id}` ? (
                    <span className="pw-copied-badge">{t("pw.copied")}</span>
                  ) : (
                    <User size={14} />
                  )}
                </button>
              )}
              <button
                type="button"
                className="pw-inline-btn"
                title={
                  type === "card"
                    ? t("pw.copy_card_number")
                    : type === "identity"
                      ? t("pw.copy_email")
                      : type === "ssh_key"
                        ? t("pw.copy_public_key")
                        : type === "note"
                          ? t("pw.copy_note")
                          : t("pw.copy_password")
                }
                aria-label={t("pw.list_copy_from_label", {
                  name: entry.service || t("pw.untitled_entry"),
                })}
                onClick={(e) => {
                  e.stopPropagation();
                  onCopyPassword(entry);
                }}
              >
                {copiedId === entry.id ? (
                  <span className="pw-copied-badge">{t("pw.copied")}</span>
                ) : (
                  <IconCopy size={14} />
                )}
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}
