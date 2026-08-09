import { useEffect, useState } from "react";
import type { PasswordEntry } from "../../lib/types";
import {
  DEFAULT_TOTP_ALGORITHM,
  DEFAULT_TOTP_DIGITS,
  DEFAULT_TOTP_PERIOD,
  generateTotp,
  totpSecondsRemaining,
} from "../../lib/totp";
import { IconCopy } from "../../ui/Icons";

type Props = {
  entry: PasswordEntry;
  now: number;
  copied: boolean;
  onCopy: (code: string) => void;
};

/** Live-updating 6(+)-digit TOTP code for one entry. Recomputes only when
 * the current time-step "bucket" changes, not on every tick, since the
 * code itself doesn't change within a period. */
export function TotpDisplay({ entry, now, copied, onCopy }: Props) {
  const period = entry.totp_period ?? DEFAULT_TOTP_PERIOD;
  const digits = entry.totp_digits ?? DEFAULT_TOTP_DIGITS;
  const algorithm = entry.totp_algorithm ?? DEFAULT_TOTP_ALGORITHM;
  const secret = entry.totp_secret ?? "";
  const bucket = Math.floor(now / 1000 / period);

  const [code, setCode] = useState("");

  useEffect(() => {
    let cancelled = false;
    generateTotp({ secret, digits, period, algorithm }, now).then((c) => {
      if (!cancelled) setCode(c);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [secret, digits, period, algorithm, bucket]);

  const secondsLeft = totpSecondsRemaining(period, now);
  const grouped =
    code.length > 3
      ? `${code.slice(0, Math.ceil(code.length / 2))} ${code.slice(Math.ceil(code.length / 2))}`
      : code;

  return (
    <div className="pw-field-row">
      <span className="pw-field-label">Code</span>
      <span className="pw-field-value pw-mask pw-totp-code">{grouped || "······"}</span>
      <span
        className="pw-totp-ring"
        style={{ "--pw-totp-frac": String(secondsLeft / period) } as React.CSSProperties}
        title={`${secondsLeft}s left`}
      />
      <button
        type="button"
        className="pw-inline-btn"
        title="Copy code"
        aria-label="Copy one-time code"
        disabled={!code}
        onClick={() => code && onCopy(code)}
      >
        {copied ? <span className="pw-copied-badge">Copied</span> : <IconCopy size={14} />}
      </button>
    </div>
  );
}
