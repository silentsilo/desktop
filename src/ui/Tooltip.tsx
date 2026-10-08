import { useEffect, useLayoutEffect, useRef, useState } from "react";

/** Hover delay before a tooltip shows. Keyboard focus shows it at once. */
export const TOOLTIP_DELAY_MS = 500;
const GAP = 6;
const EDGE = 8;
const TOOLTIP_ID = "app-tooltip";

type Tip = { text: string; target: HTMLElement };

function tooltipTarget(node: EventTarget | null): HTMLElement | null {
  return node instanceof Element ? node.closest<HTMLElement>("[data-tooltip]") : null;
}

/**
 * One tooltip for the whole window, in place of the native `title`.
 *
 * Any element with `data-tooltip` gets it: after a short hover, or as soon
 * as it takes keyboard focus. The text is the element's description too,
 * unless it only repeats its name.
 */
export function TooltipHost() {
  const [tip, setTip] = useState<Tip | null>(null);
  const [pos, setPos] = useState<{ left: number; top: number; below: boolean } | null>(null);
  const timer = useRef<number | undefined>(undefined);
  const box = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const hide = () => {
      window.clearTimeout(timer.current);
      setTip(null);
    };
    const show = (target: HTMLElement) => {
      const text = target.dataset.tooltip;
      if (text) setTip({ text, target });
    };
    const onOver = (e: PointerEvent) => {
      const target = tooltipTarget(e.target);
      if (!target) return;
      window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => show(target), TOOLTIP_DELAY_MS);
    };
    const onOut = (e: PointerEvent) => {
      const target = tooltipTarget(e.target);
      if (target && !(e.relatedTarget instanceof Node && target.contains(e.relatedTarget))) hide();
    };
    const onFocus = (e: FocusEvent) => {
      const target = tooltipTarget(e.target);
      if (target && target === e.target && target.matches(":focus-visible")) show(target);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") hide();
    };
    document.addEventListener("pointerover", onOver);
    document.addEventListener("pointerout", onOut);
    document.addEventListener("pointerdown", hide, true);
    document.addEventListener("focusin", onFocus);
    document.addEventListener("focusout", hide);
    document.addEventListener("keydown", onKey);
    window.addEventListener("scroll", hide, true);
    window.addEventListener("blur", hide);
    return () => {
      window.clearTimeout(timer.current);
      document.removeEventListener("pointerover", onOver);
      document.removeEventListener("pointerout", onOut);
      document.removeEventListener("pointerdown", hide, true);
      document.removeEventListener("focusin", onFocus);
      document.removeEventListener("focusout", hide);
      document.removeEventListener("keydown", onKey);
      window.removeEventListener("scroll", hide, true);
      window.removeEventListener("blur", hide);
    };
  }, []);

  // Placed above the element, or below when there is no room, and kept
  // inside the window.
  useLayoutEffect(() => {
    if (!tip || !box.current) {
      setPos(null);
      return;
    }
    const anchor = tip.target.getBoundingClientRect();
    const own = box.current.getBoundingClientRect();
    const below = anchor.top - own.height - GAP < EDGE;
    const top = below ? anchor.bottom + GAP : anchor.top - own.height - GAP;
    const centred = anchor.left + anchor.width / 2 - own.width / 2;
    const left = Math.min(Math.max(centred, EDGE), window.innerWidth - own.width - EDGE);
    setPos({ left, top, below });
  }, [tip]);

  // The text describes the element, unless it is the element's own name.
  useEffect(() => {
    if (!tip) return;
    const { target, text } = tip;
    const name = (target.getAttribute("aria-label") ?? target.textContent ?? "").trim();
    if (name === text) return;
    const before = target.getAttribute("aria-describedby");
    target.setAttribute("aria-describedby", before ? `${before} ${TOOLTIP_ID}` : TOOLTIP_ID);
    return () => {
      if (before) target.setAttribute("aria-describedby", before);
      else target.removeAttribute("aria-describedby");
    };
  }, [tip]);

  // A target removed while its tooltip shows takes the tooltip with it.
  useEffect(() => {
    if (!tip) return;
    const check = window.setInterval(() => {
      if (!tip.target.isConnected) setTip(null);
    }, 250);
    return () => window.clearInterval(check);
  }, [tip]);

  if (!tip) return null;
  return (
    <div
      ref={box}
      id={TOOLTIP_ID}
      role="tooltip"
      className={`tooltip${pos?.below ? " is-below" : ""}`}
      style={pos ? { left: pos.left, top: pos.top } : { left: -9999, top: -9999 }}
    >
      {tip.text}
    </div>
  );
}
