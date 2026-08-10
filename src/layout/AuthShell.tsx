import type { ReactNode } from "react";
import { BrandLogo } from "../components/BrandLogo";
import { ThemeToggle } from "../components/ThemeToggle";

type AuthShellProps = {
  title: string;
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
        <h1 className="brand-title">{title}</h1>
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
