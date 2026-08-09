import { createContext, useContext } from "react";

export type Theme = "dark" | "light";

const THEME_KEY = "theme";

/**
 * The theme to start in.
 *
 * The operating system's setting when the user has never chosen one here.
 * Defaulting to dark regardless meant someone running Windows in light mode
 * met a dark window on first launch — and, until the toggle reached the
 * screens before unlock, had no way to change it.
 */
export function preferredTheme(): Theme {
  const saved = localStorage.getItem(THEME_KEY);
  if (saved === "light" || saved === "dark") return saved;
  return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/** Whether the user has ever made this choice themselves. */
export function themeIsExplicit(): boolean {
  const saved = localStorage.getItem(THEME_KEY);
  return saved === "light" || saved === "dark";
}

/**
 * Records a choice the user made.
 *
 * Only ever called from the toggle. Writing on load as well would freeze
 * whatever the OS happened to say at first launch, turning "follow the
 * system" into a one-time snapshot of it.
 */
export function rememberTheme(theme: Theme) {
  localStorage.setItem(THEME_KEY, theme);
}

type ThemeControl = { theme: Theme; toggle: () => void };

/**
 * Exists so the shells can offer the toggle without every screen between
 * here and them carrying two props it does not otherwise use — five views,
 * seven render sites, none of which care about theming.
 */
export const ThemeContext = createContext<ThemeControl | null>(null);

export function useTheme(): ThemeControl | null {
  return useContext(ThemeContext);
}
