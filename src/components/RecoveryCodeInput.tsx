import { useCallback, useEffect, useRef, useState } from "react";
import {
  GROUP_COUNT,
  GROUP_SIZE,
  fromGroups,
  normalizeCode,
  replaceGroup,
  toGroups,
} from "../lib/recoveryCode";

type Props = {
  value: string;
  onChange: (code: string) => void;
  disabled?: boolean;
  autoFocus?: boolean;
};

/**
 * The recovery code, typed the way it was written down: one box per printed
 * group, so it can be checked against the paper group by group. The caret
 * moves on by itself and a code pasted into any box fills the rest.
 */
export function RecoveryCodeInput({ value, onChange, disabled, autoFocus }: Props) {
  // State rather than a slice of `value`: a code rebuilt from one string
  // cannot say which box an empty one is.
  const [groups, setGroups] = useState(() => toGroups(value));
  const boxes = useRef<(HTMLInputElement | null)[]>([]);

  // Only when the parent changed it. Compared as codes, so a stray dash does
  // not look like a change and wipe what is being typed.
  useEffect(() => {
    setGroups((current) =>
      normalizeCode(fromGroups(current)) === normalizeCode(value) ? current : toGroups(value)
    );
  }, [value]);

  const apply = useCallback(
    (next: string[], focus: number) => {
      setGroups(next);
      onChange(fromGroups(next));
      boxes.current[focus]?.focus();
    },
    [onChange]
  );

  const handleChange = (index: number, raw: string) => {
    const { groups: next, focus } = replaceGroup(groups, index, raw);
    apply(next, focus);
  };

  const handlePaste = (index: number, e: React.ClipboardEvent<HTMLInputElement>) => {
    // Taken over, or a whole code would be truncated into this one box.
    e.preventDefault();
    const { groups: next, focus } = replaceGroup(groups, index, e.clipboardData.getData("text"));
    apply(next, focus);
  };

  const handleKeyDown = (index: number, e: React.KeyboardEvent<HTMLInputElement>) => {
    const box = e.currentTarget;
    const atStart = box.selectionStart === 0 && box.selectionEnd === 0;

    if (e.key === "Backspace" && groups[index] === "" && index > 0) {
      // An empty box would otherwise swallow it.
      e.preventDefault();
      const next = [...groups];
      next[index - 1] = next[index - 1]!.slice(0, -1);
      apply(next, index - 1);
      return;
    }
    if (e.key === "ArrowLeft" && atStart && index > 0) {
      e.preventDefault();
      boxes.current[index - 1]?.focus();
    }
    if (e.key === "ArrowRight" && box.selectionStart === groups[index]!.length) {
      e.preventDefault();
      boxes.current[Math.min(index + 1, GROUP_COUNT - 1)]?.focus();
    }
  };

  return (
    <div className="recovery-entry" role="group" aria-label="Recovery code">
      {groups.map((group, index) => (
        <input
          key={index}
          ref={(el) => {
            boxes.current[index] = el;
          }}
          className="recovery-entry-box"
          value={group}
          onChange={(e) => handleChange(index, e.target.value)}
          onPaste={(e) => handlePaste(index, e)}
          onKeyDown={(e) => handleKeyDown(index, e)}
          onFocus={(e) => e.currentTarget.select()}
          maxLength={GROUP_SIZE}
          inputMode="text"
          spellCheck={false}
          autoComplete="off"
          autoCapitalize="characters"
          disabled={disabled}
          autoFocus={autoFocus && index === 0}
          aria-label={`Group ${index + 1} of ${GROUP_COUNT}`}
          placeholder="XXXX"
        />
      ))}
    </div>
  );
}
