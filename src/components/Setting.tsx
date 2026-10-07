import type { ReactNode } from "react";

/** Rows of settings inside a card, divided by hairlines. `separated` adds
 * a line above, for a list that follows other content in the card. */
export function SettingList({
  children,
  separated = false,
}: {
  children: ReactNode;
  separated?: boolean;
}) {
  return (
    <div className={`setting-list${separated ? " setting-list-separated" : ""}`}>{children}</div>
  );
}

type RowProps = {
  label: ReactNode;
  /** One or two short sentences under the label. */
  hint?: ReactNode;
  /** The control's id, so the label focuses or toggles it. */
  htmlFor?: string;
  /** Lines under the hint that belong to this setting: errors, status. */
  extra?: ReactNode;
  children: ReactNode;
};

/** A setting: what it is on the left, the control on the right. */
export function SettingRow({ label, hint, htmlFor, extra, children }: RowProps) {
  return (
    <div className="setting-row">
      <div className="setting-text">
        <label className="setting-label" htmlFor={htmlFor}>
          {label}
        </label>
        {hint && <p className="setting-hint">{hint}</p>}
        {extra}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}

type ToggleProps = {
  id: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (on: boolean) => void;
};

/** An on/off switch: a checkbox underneath, so keyboard and screen readers
 * get the native control. */
export function Toggle({ id, checked, disabled, onChange }: ToggleProps) {
  return (
    <span className="toggle">
      <input
        id={id}
        type="checkbox"
        role="switch"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="toggle-track" aria-hidden>
        <span className="toggle-thumb" />
      </span>
    </span>
  );
}

/** A small heading over a few related rows. */
export function SettingGroupTitle({ children }: { children: ReactNode }) {
  return <h4 className="setting-group-title">{children}</h4>;
}
