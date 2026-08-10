import type { ReactNode } from "react";
import { BrandLogo } from "../components/BrandLogo";
import { ThemeToggle } from "../components/ThemeToggle";

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
  return (
    <main className="app auth-screen">
      <div className="auth-atmosphere" aria-hidden />
      {/* Every screen before the silo opens shares this shell, so putting
          the toggle here covers the picker, unlock, enrolment, recovery and
          restore at once — all of which a person can be looking at for a
          while, and none of which could change the theme before. */}
      <ThemeToggle className="auth-theme-toggle" />
      <div className="brand">
        <BrandLogo showWordmark={false} size={56} />
        {title && <h1 className="brand-title">{title}</h1>}
        <p className="brand-sub">{subtitle}</p>
      </div>
      {children}
      {/* Only on the screens before a silo opens: these are the product's
          front door, where a publisher line belongs. Inside the app it
          would be furniture. */}
      <footer className="auth-footer" aria-label="Publisher">
        © {new Date().getFullYear()} Software Hive S.R.L.
      </footer>
    </main>
  );
}
