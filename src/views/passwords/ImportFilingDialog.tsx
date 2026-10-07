import { useState } from "react";
import { useModal } from "../../hooks/useModal";
import type { ImportCategoryChoice } from "../../lib/passwordImport";
import { t, useLocale } from "../../i18n";

type Props = {
  /** "12 logins from Bitwarden" — already counted and named by the caller. */
  what: string;
  /** Category names already in the silo, offered as filing targets. */
  categories: string[];
  onConfirm: (choice: ImportCategoryChoice) => void;
  onCancel: () => void;
};

/** The sentinel values the select uses for its two non-category rows. */
const KEEP = "\u0000keep";
const NEW = "\u0000new";

/**
 * Asks where an import should be filed before anything is stored: as the
 * file says, into one of the silo's categories, or into a new one. Exports
 * from browsers carry no folder column at all, and without this everything
 * they hold landed in "General" with no say in the matter.
 */
export function ImportFilingDialog({ what, categories, onConfirm, onCancel }: Props) {
  useLocale();
  const cardRef = useModal(onCancel);
  const [target, setTarget] = useState<string>(KEEP);
  const [fresh, setFresh] = useState("");

  const confirm = () => {
    if (target === KEEP) return onConfirm({ kind: "file" });
    if (target === NEW) return onConfirm({ kind: "into", category: fresh });
    onConfirm({ kind: "into", category: target });
  };

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div
        ref={cardRef}
        className="modal-card"
        role="dialog"
        aria-label={t("pw.filing_label")}
        onClick={(e) => e.stopPropagation()}
      >
        <h3>{t("pw.filing_title")}</h3>
        <p>{what}</p>
        <label className="field field-full">
          <span>{t("pw.field_category")}</span>
          <select value={target} onChange={(e) => setTarget(e.target.value)}>
            <option value={KEEP}>{t("pw.filing_keep")}</option>
            {categories.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
            <option value={NEW}>{t("pw.filing_new")}</option>
          </select>
        </label>
        {target === NEW && (
          <label className="field field-full">
            <span>{t("pw.field_name")}</span>
            <input
              autoFocus
              type="text"
              placeholder={t("pw.filing_placeholder")}
              value={fresh}
              onChange={(e) => setFresh(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") confirm();
              }}
            />
          </label>
        )}
        <div className="modal-actions">
          <button type="button" className="btn" onClick={onCancel}>
            {t("common.cancel")}
          </button>
          <button type="button" className="btn btn-primary" onClick={confirm}>
            {t("pw.import")}
          </button>
        </div>
      </div>
    </div>
  );
}
