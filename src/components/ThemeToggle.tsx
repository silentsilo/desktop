import { Moon, Sun } from "lucide-react";
import { useTheme } from "../lib/theme";
import { t, useLocale } from "../i18n";

/**
 * The theme switch for screens with no sidebar to put it in.
 *
 * Renders nothing when no theme control is in scope, so a screen shown
 * outside the provider degrades to having no toggle rather than crashing.
 */
export function ThemeToggle({ className = "" }: { className?: string }) {
  useLocale();
  const control = useTheme();
  if (!control) return null;

  const { theme, toggle } = control;
  return (
    <button
      type="button"
      className={`btn-theme ${className}`.trim()}
      onClick={toggle}
      title={theme === "light" ? t("nav.dark") : t("nav.light")}
      aria-label={t("start.toggle_theme")}
    >
      {theme === "light" ? <Moon size={16} /> : <Sun size={16} />}
    </button>
  );
}
