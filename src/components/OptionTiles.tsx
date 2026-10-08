import { useRef, type KeyboardEvent, type ReactNode } from "react";
import { Check } from "lucide-react";

export type OptionTile<T extends string> = {
  value: T;
  title: ReactNode;
  description?: ReactNode;
  icon?: ReactNode;
  tooltip?: string;
};

type Props<T extends string> = {
  options: OptionTile<T>[];
  /** Null when none of these is the current answer. */
  value: T | null;
  onChange: (value: T) => void;
  /** Names the group for assistive technology. */
  label: string;
  disabled?: boolean;
  /** Extra class for the grid, which sets the columns. */
  className?: string;
};

/**
 * A choice between a few kinds of thing, each a tile with a name and a
 * line about it. A radio group: one tab stop, the arrows move the choice,
 * and the chosen tile is marked by more than its colour.
 */
export function OptionTiles<T extends string>({
  options,
  value,
  onChange,
  label,
  disabled,
  className,
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
    if (step === 0) return;
    e.preventDefault();
    const next = (Math.max(index, 0) + step + options.length) % options.length;
    onChange(options[next]!.value);
    refs.current[next]?.focus();
  };

  return (
    <div
      className={`option-tiles${className ? ` ${className}` : ""}`}
      role="radiogroup"
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
            role="radio"
            aria-checked={selected}
            tabIndex={selected || (index < 0 && i === 0) ? 0 : -1}
            className={`option-tile${selected ? " is-selected" : ""}`}
            disabled={disabled}
            data-tooltip={option.tooltip}
            onClick={() => onChange(option.value)}
          >
            {option.icon && <span className="option-tile-icon">{option.icon}</span>}
            <span className="option-tile-text">
              <strong>{option.title}</strong>
              {option.description && <span>{option.description}</span>}
            </span>
            {selected && <Check className="option-tile-check" size={16} aria-hidden />}
          </button>
        );
      })}
    </div>
  );
}
