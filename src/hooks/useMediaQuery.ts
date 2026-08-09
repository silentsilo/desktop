import { useEffect, useState } from "react";

/**
 * As much of a `MediaQueryList` as watching one needs. Spelled out rather
 * than picked off the DOM type, whose overloaded `addEventListener` a plain
 * stand-in cannot satisfy.
 */
type Watchable = {
  readonly matches: boolean;
  addEventListener(type: "change", listener: () => void): void;
  removeEventListener(type: "change", listener: () => void): void;
};

/**
 * Reports whether a media query matches, now and whenever that changes.
 *
 * Takes the list rather than the query string, so the subscription can be
 * tested without a window: the tests here run in Node, and the substance
 * worth testing is the reading and the teardown, not `matchMedia` itself.
 */
export function watchMediaQuery(list: Watchable, onMatch: (matches: boolean) => void): () => void {
  const read = () => onMatch(list.matches);
  // Read on subscribe, not only on change: the viewport can move between the
  // render that decided the initial value and the effect that subscribes,
  // and that miss would stick until the user resized again.
  read();
  list.addEventListener("change", read);
  return () => list.removeEventListener("change", read);
}

/**
 * Tracks a CSS media query from React.
 *
 * For layout that has to change in JavaScript, not just in CSS: a sidebar
 * that collapses below a width has to stop rendering its labels and its
 * toggle, and a stylesheet cannot decide that on its own.
 */
export function useMediaQuery(query: string): boolean {
  // `matchMedia` is optional-chained the way the theme code does it, for the
  // environments that have a window without one.
  const [matches, setMatches] = useState(() => window.matchMedia?.(query).matches ?? false);

  useEffect(() => {
    const list = window.matchMedia?.(query);
    if (!list) return;
    return watchMediaQuery(list, setMatches);
  }, [query]);

  return matches;
}
