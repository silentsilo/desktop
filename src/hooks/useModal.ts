import { useEffect, useRef } from "react";

/**
 * The parts of a dialog that every dialog was implementing differently.
 *
 * Before this, Escape closed the new-folder modal but not the password
 * editor, the confirmation or either shell browser; clicking the backdrop
 * closed three of six; and Tab walked straight out of the dialog into the
 * page behind it, which for a modal asking "delete these files permanently?"
 * means the focus ring is somewhere the user cannot see.
 *
 * Returns the ref to put on the dialog card. Pair it with `role="dialog"`
 * and `aria-modal`.
 *
 * `open` exists for the dialogs rendered inline by a parent that stays
 * mounted around them: the card only enters the DOM when it opens, so the
 * setup has to run then rather than when the parent first renders.
 */
export function useModal(onClose: (() => void) | undefined, open = true) {
  const ref = useRef<HTMLDivElement>(null);
  // Read by the key handler, which is registered once: re-binding it on every
  // render would drop keystrokes mid-dialog.
  const closeRef = useRef(onClose);
  useEffect(() => {
    closeRef.current = onClose;
  });

  useEffect(() => {
    if (!open) return;
    const card = ref.current;
    if (!card) return;

    // Whatever had focus when the dialog opened gets it back on close —
    // otherwise focus falls to the top of the document and a keyboard user
    // has to tab back to where they were.
    const previous = document.activeElement as HTMLElement | null;

    const focusable = () =>
      [
        ...card.querySelectorAll<HTMLElement>(
          'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
        ),
      ].filter((el) => el.offsetParent !== null);

    // Only when nothing inside asked for focus itself: several dialogs
    // autofocus a text field, and stealing that would put the caret nowhere.
    if (!card.contains(document.activeElement)) {
      focusable()[0]?.focus();
    }

    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        closeRef.current?.();
        return;
      }
      if (e.key !== "Tab") return;
      const items = focusable();
      if (items.length === 0) return;
      const first = items[0]!;
      const last = items[items.length - 1]!;
      const active = document.activeElement;
      // Wrap at both ends, and pull focus back in if it has already escaped.
      if (e.shiftKey && (active === first || !card.contains(active))) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && (active === last || !card.contains(active))) {
        e.preventDefault();
        first.focus();
      }
    };

    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("keydown", onKeyDown, true);
      previous?.focus?.();
    };
  }, [open]);

  return ref;
}
