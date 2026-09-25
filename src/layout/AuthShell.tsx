import type { ReactNode } from "react";
import { Settings2 } from "lucide-react";
import { BrandLogo } from "../components/BrandLogo";
import { ThemeToggle } from "../components/ThemeToggle";
import { useOpenAppSettings } from "../lib/appSettings";

type AuthShellProps = {
  /**
   * The app's name, and only where the app is introducing itself: the
   * picker a new install opens on, and the moment before it. Everywhere
   * else the card below already says what the screen is for, the window
   * title says what the app is, and the mark above says it again, so a
   * third copy costs a line and tells nobody anything.
   */
  title?: string;
  subtitle: string;
  children: ReactNode;
};

export function AuthShell({ title, subtitle, children }: AuthShellProps) {
  const openSettings = useOpenAppSettings();
  return (
    <main className="app auth-screen">
      <div className="auth-atmosphere" aria-hidden />
      {/* Every screen before the silo opens shares this shell, so putting
          the toggle here covers the picker, unlock, enrolment, recovery and
          restore at once — all of which a person can be looking at for a
          while, and none of which could change the theme before. */}
      <div className="auth-corner">
        {/* Updates, startup and the browser extension belong to the app, and
            an update toast says to install from Settings: they have to be
            reachable before any silo is unlocked. */}
        {openSettings && (
          <button
            type="button"
            className="btn-theme"
            onClick={openSettings}
            title="App settings"
            aria-label="App settings"
          >
            <Settings2 size={16} />
          </button>
        )}
        <ThemeToggle />
      </div>
      <div className="brand">
        <BrandLogo showWordmark={false} size={56} />
        {title && <h1 className="brand-title">{title}</h1>}
        <p className="brand-sub">{subtitle}</p>
      </div>
      {children}
      {/* Only on the screens before a silo opens: these are the product's
          front door, where a publisher line belongs. Inside the app it
          would be furniture. The version is here so a screenshot sent for
          help says which one it is; inside, the sidebar shows it. */}
      <footer className="auth-footer" aria-label="Publisher">
        © {new Date().getFullYear()} Software Hive S.R.L. · v{__APP_VERSION__}
      </footer>
    </main>
  );
}
