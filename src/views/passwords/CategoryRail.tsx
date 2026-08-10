import { useState } from "react";
import { Check, Contact, CreditCard, KeyRound, StickyNote, TerminalSquare } from "lucide-react";
import type { CredentialType, PasswordCategory } from "../../lib/types";
import { CREDENTIAL_TYPES, FALLBACK_CATEGORY, hashColor, TYPE_LABELS } from "./util";
import { IconClose, IconEdit, IconPlus, IconTrash } from "../../ui/Icons";

export const TYPE_ICONS: Record<CredentialType, typeof KeyRound> = {
  login: KeyRound,
  card: CreditCard,
  identity: Contact,
  ssh_key: TerminalSquare,
  note: StickyNote,
};

type Props = {
  categories: PasswordCategory[];
  counts: Map<string, number>;
  total: number;
  selected: string | null;
  typeCounts: Map<CredentialType, number>;
  selectedType: CredentialType | null;
  onSelectType: (type: CredentialType | null) => void;
  busy: boolean;
  onSelect: (name: string | null) => void;
  onAdd: (category: PasswordCategory) => void;
  onRename: (from: string, to: string) => void;
  onDelete: (name: string) => void;
};

/**
 * The left rail: filter and manage in one place.
 *
 * Management lives on the items themselves (a pencil on hover) rather than
 * in a settings dialog, because renaming a category is something done while
 * looking at it, with the counts right there saying what the change will
 * touch.
 */
export function CategoryRail({
  categories,
  counts,
  total,
  selected,
  typeCounts,
  selectedType,
  onSelectType,
  busy,
  onSelect,
  onAdd,
  onRename,
  onDelete,
}: Props) {
  const [adding, setAdding] = useState(false);
  const [draft, setDraft] = useState("");
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [confirmingDelete, setConfirmingDelete] = useState<string | null>(null);

  const nameTaken = (name: string, besides?: string) =>
    categories.some(
      (c) => c.name.toLowerCase() === name.toLowerCase() && c.name !== besides
    );

  const submitAdd = () => {
    const name = draft.trim();
    if (!name || nameTaken(name)) return;
    onAdd({ name, color: hashColor(name) });
    setDraft("");
    setAdding(false);
  };

  const submitRename = () => {
    const to = renameDraft.trim();
    if (!renaming || !to || nameTaken(to, renaming)) return;
    if (to !== renaming) onRename(renaming, to);
    setRenaming(null);
  };

  return (
    <nav className="view-rail" aria-label="Credential types and categories">
      {/* Kinds first: "show me my cards" is the question asked walking up
          to a checkout, and it should not require remembering a category. */}
      <button
        type="button"
        className={`view-rail-item${selectedType === null ? " active" : ""}`}
        onClick={() => onSelectType(null)}
      >
        <span className="view-rail-label">All items</span>
        <span className="view-rail-count">{total}</span>
      </button>
      {CREDENTIAL_TYPES.map((type) => {
        const Icon = TYPE_ICONS[type];
        return (
          <button
            key={type}
            type="button"
            className={`view-rail-item${selectedType === type ? " active" : ""}`}
            onClick={() => onSelectType(selectedType === type ? null : type)}
          >
            <Icon size={14} aria-hidden className="pw-rail-type-icon" />
            <span className="view-rail-label">{TYPE_LABELS[type].plural}</span>
            <span className="view-rail-count">{typeCounts.get(type) ?? 0}</span>
          </button>
        );
      })}

      <div className="view-rail-heading">Categories</div>
      <button
        type="button"
        className={`view-rail-item${selected === null ? " active" : ""}`}
        onClick={() => onSelect(null)}
      >
        <span className="view-rail-label">All</span>
        <span className="view-rail-count">{total}</span>
      </button>

      {categories.map((cat) =>
        renaming === cat.name ? (
          <div key={cat.name}>
            <div className="pw-rail-edit">
              <input
                type="text"
                value={renameDraft}
                autoFocus
                aria-label={`Rename ${cat.name}`}
                onChange={(e) => setRenameDraft(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") submitRename();
                  if (e.key === "Escape") setRenaming(null);
                }}
              />
              <button
                type="button"
                className="pw-inline-btn"
                title="Save name"
                disabled={
                  !renameDraft.trim() || nameTaken(renameDraft.trim(), cat.name)
                }
                onClick={submitRename}
              >
                <Check size={14} />
              </button>
              <button
                type="button"
                className="pw-inline-btn"
                title="Cancel"
                onClick={() => setRenaming(null)}
              >
                <IconClose size={14} />
              </button>
            </div>
            {nameTaken(renameDraft.trim(), cat.name) && (
              <p className="hint pw-rail-hint">That name is already in the list.</p>
            )}
          </div>
        ) : (
          <div
            key={cat.name}
            className={`pw-rail-row${selected === cat.name ? " active" : ""}`}
          >
            <button
              type="button"
              className="view-rail-item"
              onClick={() => onSelect(selected === cat.name ? null : cat.name)}
            >
              <span className="pw-rail-dot" style={{ background: cat.color }} />
              <span className="view-rail-label">{cat.name}</span>
              <span className="view-rail-count">{counts.get(cat.name) ?? 0}</span>
            </button>
            {confirmingDelete === cat.name ? (
              <div className="pw-rail-tools is-open">
                <button
                  type="button"
                  className="pw-inline-btn danger"
                  title={`Delete ${cat.name}. Its entries move to ${FALLBACK_CATEGORY}.`}
                  onClick={() => {
                    setConfirmingDelete(null);
                    onDelete(cat.name);
                  }}
                >
                  <Check size={14} />
                </button>
                <button
                  type="button"
                  className="pw-inline-btn"
                  title="Keep it"
                  onClick={() => setConfirmingDelete(null)}
                >
                  <IconClose size={14} />
                </button>
              </div>
            ) : (
              <div className="pw-rail-tools">
                <button
                  type="button"
                  className="pw-inline-btn"
                  title="Rename category"
                  disabled={busy}
                  onClick={() => {
                    setRenameDraft(cat.name);
                    setRenaming(cat.name);
                  }}
                >
                  <IconEdit size={13} />
                </button>
                {/* The fallback stays: deleting a category needs somewhere
                    to put the entries it orphans. */}
                {cat.name !== FALLBACK_CATEGORY && (
                  <button
                    type="button"
                    className="pw-inline-btn danger"
                    title="Delete category"
                    disabled={busy}
                    onClick={() => setConfirmingDelete(cat.name)}
                  >
                    <IconTrash size={13} />
                  </button>
                )}
              </div>
            )}
          </div>
        )
      )}

      {adding ? (
        <>
          <div className="pw-rail-edit">
            <input
              type="text"
              value={draft}
              autoFocus
              placeholder="Category name"
              aria-label="New category name"
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") submitAdd();
                if (e.key === "Escape") {
                  setAdding(false);
                  setDraft("");
                }
              }}
            />
            <button
              type="button"
              className="pw-inline-btn"
              title="Add category"
              disabled={!draft.trim() || nameTaken(draft.trim())}
              onClick={submitAdd}
            >
              <Check size={14} />
            </button>
            <button
              type="button"
              className="pw-inline-btn"
              title="Cancel"
              onClick={() => {
                setAdding(false);
                setDraft("");
              }}
            >
              <IconClose size={14} />
            </button>
          </div>
          {/* A disabled check with no explanation reads as a broken button.
              Say which rule the name is failing while it is being typed. */}
          {nameTaken(draft.trim()) && (
            <p className="hint pw-rail-hint">That name is already in the list.</p>
          )}
        </>
      ) : (
        <button
          type="button"
          className="pw-rail-add"
          disabled={busy}
          onClick={() => setAdding(true)}
        >
          <IconPlus size={14} />
          <span>New category</span>
        </button>
      )}
    </nav>
  );
}
