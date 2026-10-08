import { useRef, type KeyboardEvent, type ReactNode } from "react";

export type SegmentedOption<T extends string> = {
  value: T;
  label: ReactNode;
  icon?: ReactNode;
  /** Shown after the label, such as an update badge. */
  extra?: ReactNode;
  tooltip?: string;
};

type Props<T extends string> = {
  options: SegmentedOption<T>[];
  value: T;
  onChange: (value: T) => void;
  /** Names the group for assistive technology. */
  label: string;
  /** "tab" when the choice switches what is shown below it. */
  kind?: "radio" | "tab";
  disabled?: boolean;
  /** Each option takes an equal share of the width. */
  fill?: boolean;
};

/**
 * A choice between a few answers, one of which is always picked.
 *
 * One tab stop for the group; the arrow keys move the choice, like the
 * native radio group it stands in for.
 */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  label,
  kind = "radio",
  disabled,
  fill,
}: Props<T>) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const index = options.findIndex((o) => o.value === value);

  const onKeyDown = (e: KeyboardEvent) => {
    const step =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (step === 0 && e.key !== "Home" && e.key !== "End") return;
    e.preventDefault();
    const next =
      e.key === "Home"
        ? 0
        : e.key === "End"
          ? options.length - 1
          : (Math.max(index, 0) + step + options.length) % options.length;
    onChange(options[next]!.value);
    refs.current[next]?.focus();
  };

  return (
    <div
      className={`segmented${fill ? " is-fill" : ""}`}
      role={kind === "tab" ? "tablist" : "radiogroup"}
      aria-label={label}
      onKeyDown={onKeyDown}
    >
      {options.map((option, i) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            ref={(el) => {
              refs.current[i] = el;
            }}
            type="button"
            role={kind}
            aria-checked={kind === "radio" ? selected : undefined}
            aria-selected={kind === "tab" ? selected : undefined}
            tabIndex={selected || (index < 0 && i === 0) ? 0 : -1}
            className={`segmented-item${selected ? " is-selected" : ""}`}
            disabled={disabled}
            data-tooltip={option.tooltip}
            onClick={() => onChange(option.value)}
          >
            {option.icon}
            <span>{option.label}</span>
            {option.extra}
          </button>
        );
      })}
    </div>
  );
}
