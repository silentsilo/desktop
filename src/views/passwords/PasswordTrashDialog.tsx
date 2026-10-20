import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useModal } from "../../hooks/useModal";
import { formatDay } from "../../lib/format";
import { formatAppError } from "../../lib/errors";
import type { CredentialType } from "../../lib/types";
import { t, useLocale } from "../../i18n";
import { TYPE_ICONS } from "./CategoryRail";

/** One entry in the password trash, as `vault_password_trash` sends it:
 *  what the list shows, never the secrets. */
export type TrashedEntry = {
  id: string;
  deleted_at: number;
  service: string;
  username: string;
  url: string;
  kind: string;
};

type Props = {
  /** Puts one back; resolves once the silo has it, rejects with the error. */
  onRestore: (entry: TrashedEntry) => Promise<void>;
  onClose: () => void;
};

/**
 * Entries deleted in the last 30 days, each with "Put back". Read from the
 * silo each time it opens: the trash follows deletes made on other devices.
 */
export function PasswordTrashDialog({ onRestore, onClose }: Props) {
  useLocale();
  const cardRef = useModal(onClose);
  const [items, setItems] = useState<TrashedEntry[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setItems(await invoke<TrashedEntry[]>("vault_password_trash"));
    } catch (e) {
      setError(formatAppError(e));
      setItems([]);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const restore = async (item: TrashedEntry) => {
    setError(null);
    setBusyId(item.id);
    try {
      await onRestore(item);
      setItems((held) => (held ?? []).filter((i) => i.id !== item.id));
    } catch (e) {
      setError(formatAppError(e));
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div
        ref={cardRef}
        className="modal-card pw-trash-card"
        role="dialog"
        aria-label={t("pw.trash_title")}
        onClick={(e) => e.stopPropagation()}
      >
        <h3>{t("pw.trash_title")}</h3>
        <p className="hint">{t("pw.trash_hint")}</p>
        {error && (
          <p className="hint pw-totp-hint-error" role="alert">
            {error}
          </p>
        )}
        {items === null ? null : items.length === 0 ? (
          <p className="pw-trash-empty">{t("pw.trash_empty")}</p>
        ) : (
          <ul className="pw-trash-list">
            {items.map((item) => {
              const Icon = TYPE_ICONS[(item.kind || "login") as CredentialType] ?? TYPE_ICONS.login;
              return (
                <li key={item.id} className="pw-trash-row">
                  <Icon size={16} className="pw-trash-icon" />
                  <div className="pw-trash-text">
                    <strong>{item.service || t("pw.trash_unnamed")}</strong>
                    <span>
                      {[item.username, t("pw.trash_deleted_on", { date: formatDay(item.deleted_at) })]
                        .filter(Boolean)
                        .join(" · ")}
                    </span>
                  </div>
                  <button
                    type="button"
                    className="btn-secondary"
                    disabled={busyId !== null}
                    onClick={() => void restore(item)}
                  >
                    {t("pw.trash_restore")}
                  </button>
                </li>
              );
            })}
          </ul>
        )}
        <div className="modal-actions">
          <button type="button" className="btn-secondary" onClick={onClose}>
            {t("dlg.close")}
          </button>
        </div>
      </div>
    </div>
  );
}
