import { useEffect, useState } from "react";
import { Lock } from "lucide-react";
import { t, useLocale } from "../i18n";
import { countdown } from "../lib/lockNotice";

/**
 * The last minute before the silo on screen locks itself, counted down.
 * Shown only then: any use resets the timer, so a countdown on screen at
 * other times would sit still at its full value. Moving the mouse or
 * pressing a key takes it away, which is also what keeps the silo open.
 */
export function LockSoonBar({ name, deadline }: { name: string; deadline: number }) {
  useLocale();
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  return (
    <div className="lock-soon" role="status" aria-live="polite">
      <Lock size={16} aria-hidden />
      <div>
        <strong>{t("app.lock_soon", { name, time: countdown((deadline - now) / 1000) })}</strong>
        <span>{t("app.lock_soon_hint")}</span>
      </div>
    </div>
  );
}
