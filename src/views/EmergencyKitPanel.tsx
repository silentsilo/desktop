import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { AlertTriangle, Printer } from "lucide-react";
import { EmergencyKit } from "./EmergencyKit";

type Props = {
  busy: boolean;
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
export function EmergencyKitPanel({ busy, siloName, freshCode }: Props) {
  /// The preview is the sheet at its real size, shrunk to fit the pane.
  ///
  /// It has to be the real size first: everything on the page is measured in
  /// millimetres, so a sheet squeezed into a narrower box keeps the boxes and
  /// rules at their printed dimensions and they run off the edge. Rendering
  /// at full width and scaling the result keeps the preview a true picture of
  /// the paper, which is the only thing it is for.
  const previewRef = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(1);

  useEffect(() => {
    const box = previewRef.current;
    if (!box) return;
    const probe = document.createElement("div");
    probe.style.cssText = "position:absolute;visibility:hidden;width:194mm";
    document.body.appendChild(probe);
    const sheetWidth = probe.getBoundingClientRect().width;
    probe.remove();
    if (sheetWidth === 0) return;

    const fit = () => setScale(Math.min(1, box.clientWidth / sheetWidth));
    fit();
    const observer = new ResizeObserver(fit);
    observer.observe(box);
    return () => observer.disconnect();
  }, []);

  const [mode, setMode] = useState<"blank" | "printed">("blank");
  const [typed, setTyped] = useState("");

  // Whatever the user has: the code they just generated, or one they typed
  // back in from an existing sheet. Blank prints empty boxes.
  const code = mode === "blank" ? "" : (freshCode ?? typed).trim();
  const needsTyping = mode === "printed" && !freshCode;
  const looksComplete = code.replace(/-/g, "").length === 32;

  return (
    <div className="kit-block">
      <h4>
        <Printer size={16} />
        Print an emergency kit
      </h4>
      <p>
        One sheet of paper holding everything needed to get this silo back on a computer that has
        never seen it: the recovery code, where your files are, and what to do, in order. Print
        it and keep it where you keep documents you cannot replace.
      </p>
      <p className="hint">
        No file is written. The page goes straight to your printer through the usual dialog, so
        the code does not land on this disk on the way. If you choose "Save as PDF" in that
        dialog, that is your decision rather than ours, and the file it makes is worth exactly as
        much as the sheet would be.
      </p>

      <div className="field">
        <span>How should the code get onto the paper?</span>
        <div className="storage-choice kit-choice">
          <button
            type="button"
            className={`storage-option${mode === "blank" ? " is-chosen" : ""}`}
            onClick={() => setMode("blank")}
          >
            <strong>Leave the boxes empty</strong>
            <span>
              You copy the code in by hand. Nothing goes through the printer, which on a shared
              or office machine means no spooler, no queue and no log. The safer one.
            </span>
          </button>
          <button
            type="button"
            className={`storage-option${mode === "printed" ? " is-chosen" : ""}`}
            onClick={() => setMode("printed")}
          >
            <strong>Print the code too</strong>
            <span>
              Faster and impossible to transcribe wrongly. Reasonable on a printer in your own
              home, and worth avoiding on one you do not control.
            </span>
          </button>
        </div>
      </div>

      {needsTyping && (
        <label className="field">
          <span>Your recovery code</span>
          <input
            value={typed}
            disabled={busy}
            spellCheck={false}
            placeholder="Type it from your existing sheet"
            onChange={(e) => setTyped(e.target.value)}
          />
          <span className="hint">
            The app never keeps your code, only a sealed copy of the key it opens, so it cannot
            fill this in for you. If you no longer have it, generate a new one under Recovery
            code first: the old one stops working when you do.
          </span>
        </label>
      )}

      {mode === "printed" && code.length > 0 && !looksComplete && (
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          That is not a whole code. It should come to 32 letters and digits.
        </p>
      )}

      <div className="actions">
        <button
          type="button"
          disabled={busy || (mode === "printed" && !looksComplete)}
          onClick={() => window.print()}
        >
          <Printer size={15} />
          Print the kit
        </button>
      </div>

      <p className="hint">
        In the print dialog, turn <strong>Headers and footers</strong> off. Left on, the printer
        adds today's date and an internal address along the top and bottom, which belongs on a
        web page and not on this.
      </p>

      <p className="hint">Below is exactly what will come out of the printer.</p>

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
