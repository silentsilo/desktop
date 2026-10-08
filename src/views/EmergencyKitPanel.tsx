import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AlertTriangle, Printer } from "lucide-react";
import { EmergencyKit } from "./EmergencyKit";
import { fromGroups, isComplete, toGroups } from "../lib/recoveryCode";
import { markDone } from "../lib/siloMemory";
import { RecoveryCodeInput } from "../components/RecoveryCodeInput";
import { OptionTiles } from "../components/OptionTiles";
import { t, tx, useLocale } from "../i18n";

type Props = {
  busy: boolean;
  /** For remembering when the kit was last printed, for the overview. */
  siloId: string;
  siloName: string;
  /**
   * A code generated in this session, if there was one. The app never stores
   * the code itself, only the envelope, so this is the one moment it can be
   * printed without asking for it back.
   */
  freshCode: string | null;
};

/**
 * Printing the sheet that gets a silo back.
 *
 * Part of the recovery card rather than a card of its own: the code and the
 * paper it goes on are one subject, and whether to print is a decision about
 * the same thing rather than a separate feature.
 *
 * Two ways to fill in the code, and the safer one is not the default only
 * because it is the more laborious one. Printing it puts the code through the
 * printer, which on an office machine means a spooler, a queue and possibly a
 * log. Writing it in by hand puts it nowhere but the paper.
 */
export function EmergencyKitPanel({ busy, siloId, siloName, freshCode }: Props) {
  /// The preview is the sheet at its real size, shrunk to fit the pane.
  ///
  /// It has to be the real size first: everything on the page is measured in
  /// millimetres, so a sheet squeezed into a narrower box keeps the boxes and
  /// rules at their printed dimensions and they run off the edge. Rendering
  /// at full width and scaling the result keeps the preview a true picture of
  /// the paper, which is the only thing it is for.
  const locale = useLocale();
  const previewRef = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);

  useEffect(() => {
    const box = previewRef.current;
    if (!box) return;
    const probe = document.createElement("div");
    // 210mm for the languages printed at full width, as the stylesheet says.
    const wide = locale !== "en" && locale !== "ro";
    probe.style.cssText = `position:absolute;visibility:hidden;width:${wide ? 210 : 194}mm`;
    document.body.appendChild(probe);
    const sheetWidth = probe.getBoundingClientRect().width;
    probe.remove();
    if (sheetWidth === 0) return;

    const fit = () => setScale(Math.min(1, box.clientWidth / sheetWidth));
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(box);
    return () => observer.disconnect();
  }, [locale]);

  const [mode, setMode] = useState<"blank" | "printed">("blank");
  const [typed, setTyped] = useState("");

  // Whatever the user has: the code they just generated, or one they typed
  // back in from an existing sheet. Blank prints empty boxes.
  // A typed code is folded the way unlock reads one (case, spaces, and the
  // letters Crockford's alphabet leaves out) and printed in the eight groups
  // of the original. Taken as typed, lowercase and spaces went onto the
  // paper, and a correct code typed with spaces was refused as incomplete.
  const source = freshCode ?? typed;
  const code = mode === "blank" ? "" : fromGroups(toGroups(source));
  const needsTyping = mode === "printed" && !freshCode;
  const looksComplete = isComplete(source);

  return (
    <div className="kit-block">
      <h4>
        <Printer size={16} />
        {t("kit.panel_title")}
      </h4>
      <p>{t("kit.panel_intro")}</p>
      <p className="hint">{t("kit.panel_no_file")}</p>

      <div className="field">
        <span>{t("kit.how_question")}</span>
        <OptionTiles
          className="kit-choice"
          label={t("kit.how_question")}
          value={mode}
          onChange={setMode}
          options={[
            { value: "blank", title: t("kit.blank_title"), description: t("kit.blank_body") },
            {
              value: "printed",
              title: t("kit.printed_title"),
              description: t("kit.printed_body"),
            },
          ]}
        />
      </div>

      {needsTyping && (
        <div className="field">
          <span>{t("kit.typed_label")}</span>
          <RecoveryCodeInput value={typed} disabled={busy} onChange={setTyped} />
          <span className="hint">{t("kit.typed_hint")}</span>
        </div>
      )}

      {mode === "printed" && code.length > 0 && !looksComplete && (
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          {t("kit.not_whole")}
        </p>
      )}

      <div className="actions">
        <button
          className="btn-primary"
          type="button"
          disabled={busy || (mode === "printed" && !looksComplete)}
          onClick={() => {
            // Whether the dialog ended in paper is not something the app
            // can know, so opening it counts.
            markDone(siloId, "kit-printed");
            window.print();
          }}
        >
          <Printer size={15} />
          {t("kit.print")}
        </button>
      </div>

      <p className="hint">
        {tx("kit.headers_hint", { setting: <strong>{t("kit.headers_label")}</strong> })}
      </p>

      <p className="hint">{t("kit.preview_hint")}</p>

      <div className="kit-preview" ref={previewRef}>
        <div className="kit-preview-scale" style={{ zoom: scale }}>
          <EmergencyKit siloName={siloName} code={code} />
        </div>
      </div>

      {/* The copy that actually prints, put directly under <body>.
          `.panel-section` creates a containing block, so a sheet positioned
          inside it anchors to the panel rather than to the page and comes out
          shifted down the paper. A direct child of body has nothing to
          anchor to. */}
      {createPortal(
        <EmergencyKit siloName={siloName} code={code} variant="kit-print" />,
        document.body,
      )}
    </div>
  );
}
