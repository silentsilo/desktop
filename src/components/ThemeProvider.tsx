import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  preferredTheme,
  rememberTheme,
  ThemeContext,
  themeIsExplicit,
  type Theme,
} from "../lib/theme";

/**
 * Holds the theme above the app rather than inside it.
 *
 * The app returns early for each screen before a silo opens — picker,
 * unlock, enrolment, recovery, restore — so state living inside it would
 * have to be threaded into every one of those branches to reach them. One
 * provider above the whole tree reaches all of them, including the ones
 * added later.
 */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(preferredTheme);

  useEffect(() => {
    document.documentElement.className = theme === "light" ? "light-theme" : "";
  }, [theme]);

  /// Follows the system until the user overrides it.
  ///
  /// Never having touched the toggle means "whatever the computer is
  /// doing", including when that changes at sunset. Having touched it means
  /// what they picked, which the system does not get to overrule afterwards.
  useEffect(() => {
    const media = window.matchMedia?.("(prefers-color-scheme: light)");
    if (!media) return;
    const follow = (e: MediaQueryListEvent) => {
      if (themeIsExplicit()) return;
      setTheme(e.matches ? "light" : "dark");
    };
    media.addEventListener("change", follow);
    return () => media.removeEventListener("change", follow);
  }, []);

  const toggle = useCallback(() => {
    setTheme((current) => {
      const next = current === "light" ? "dark" : "light";
      // Written here and nowhere else: persisting on load as well would
      // freeze whatever the system happened to say at first launch, turning
      // "follow the system" into a one-time snapshot of it.
      rememberTheme(next);
      return next;
    });
  }, []);

  const value = useMemo(() => ({ theme, toggle }), [theme, toggle]);
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}
