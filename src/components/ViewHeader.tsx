import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

type Props = {
  icon: LucideIcon;
  title: string;
  /** One line saying what this page is, or what it currently holds. */
  subtitle?: string;
  /** Anything that belongs to the page as a whole rather than to a pane. */
  actions?: ReactNode;
};

/**
 * The header the pane views share.
 *
 * The shell used to print a bare `<h2>` above these pages, which the pane
 * layouts then had to work around, so they dropped it and lost their names
 * altogether. This gives the name back in a row that carries its own
 * weight: the view's icon, what the page is, and one line of what is in it.
 */
export function ViewHeader({ icon: Icon, title, subtitle, actions }: Props) {
  return (
    <header className="view-header">
      <span className="view-header-icon" aria-hidden>
        <Icon size={18} />
      </span>
      <div className="view-header-text">
        <h2>{title}</h2>
        {subtitle && <p>{subtitle}</p>}
      </div>
      {actions && <div className="view-header-actions">{actions}</div>}
    </header>
  );
}
