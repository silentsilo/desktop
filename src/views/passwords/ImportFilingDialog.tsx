import { useState } from "react";
import { useModal } from "../../hooks/useModal";
import type { ImportCategoryChoice } from "../../lib/passwordImport";

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
        aria-label="Choose a category for the import"
        onClick={(e) => e.stopPropagation()}
      >
        <h3>Where should these go?</h3>
        <p>{what}</p>
        <label className="field field-full">
          <span>Category</span>
          <select value={target} onChange={(e) => setTarget(e.target.value)}>
            <option value={KEEP}>Keep the categories from the file</option>
            {categories.map((name) => (
              <option key={name} value={name}>
                {name}
              </option>
            ))}
            <option value={NEW}>New category…</option>
          </select>
        </label>
        {target === NEW && (
          <label className="field field-full">
            <span>Name</span>
            <input
              autoFocus
              type="text"
              placeholder="e.g. Imported"
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
            Cancel
          </button>
          <button type="button" className="btn btn-primary" onClick={confirm}>
            Import
          </button>
        </div>
      </div>
    </div>
  );
}
