import { FileSpreadsheet, KeyRound, ShieldCheck } from "lucide-react";
import { useModal } from "../../hooks/useModal";
import { t, useLocale } from "../../i18n";

export type ExportKind = "silentsilo" | "kdbx" | "csv";

type Props = {
  /** How many ticked entries go, or null for all of them. */
  selected: number | null;
  busy: boolean;
  onChoose: (kind: ExportKind) => void;
  onCancel: () => void;
};

/**
 * Which file to export to: one card per kind, the one that brings every
 * entry back exactly first, the unencrypted one last and in the warning
 * colours. A choice starts that export at once.
 */
export function ExportChoiceDialog({ selected, busy, onChoose, onCancel }: Props) {
  useLocale();
  const cardRef = useModal(onCancel);
  const options: {
    kind: ExportKind;
    Icon: typeof ShieldCheck;
    name: string;
    text: string;
    tone?: "recommended" | "plain";
  }[] = [
    {
      kind: "silentsilo",
      Icon: ShieldCheck,
      name: t("pw.export_sse_button"),
      text: t("pw.export_sse_short"),
      tone: "recommended",
    },
    { kind: "kdbx", Icon: KeyRound, name: t("pw.export_kdbx_button"), text: t("pw.export_kdbx_short") },
    {
      kind: "csv",
      Icon: FileSpreadsheet,
      name: t("pw.export_csv_name"),
      text: t("pw.export_csv_short"),
      tone: "plain",
    },
  ];

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div
        ref={cardRef}
        className="modal-card pw-export-card"
        role="dialog"
        aria-modal="true"
        aria-label={t("pw.export_choose_label")}
        onClick={(e) => e.stopPropagation()}
      >
        <h3>
          {selected === null
            ? t("pw.export_choose_all")
            : t("pw.export_choose_selected", { count: selected })}
        </h3>
        <div className="pw-export-options">
          {options.map(({ kind, Icon, name, text, tone }) => (
            <button
              key={kind}
              type="button"
              className={`pw-export-option${tone ? ` is-${tone}` : ""}`}
              disabled={busy}
              onClick={() => onChoose(kind)}
            >
              <Icon size={20} aria-hidden />
              <span className="pw-export-option-name">
                {name}
                {tone === "recommended" && <em>{t("pw.export_recommended")}</em>}
              </span>
              <span className="pw-export-option-text">{text}</span>
            </button>
          ))}
        </div>
        <div className="modal-actions">
          <button type="button" className="btn-secondary" onClick={onCancel}>
            {t("common.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
