import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { KeyRound } from "lucide-react";
import { useEventSubscription } from "../hooks/useEventSubscription";

/** What the key asked, as core sends it. */
type PinAsk = { kind: "enter" | "wrong"; retries: number | null };

/**
 * The security key's PIN, on Linux and macOS, where the app talks to the
 * key itself. Windows asks in its own dialog. The PIN goes to the key and
 * nowhere else; nothing here keeps it.
 */
export function SecurityKeyPinDialog() {
  const [ask, setAsk] = useState<PinAsk | null>(null);
  const [pin, setPin] = useState("");

  useEventSubscription(
    () =>
      listen<PinAsk>("fido-pin-request", (event) => {
        setPin("");
        setAsk(event.payload);
      }),
    [],
  );
  useEventSubscription(() => listen("fido-pin-done", () => setAsk(null)), []);

  if (!ask) return null;

  const answer = (value: string | null) => {
    setAsk(null);
    setPin("");
    void invoke("fido_pin_answer", { pin: value }).catch(() => {});
  };

  const left =
    ask.retries === null
      ? ""
      : ask.retries === 1
        ? " One try left before the key blocks its PIN."
        : ` ${ask.retries} tries left.`;

  return (
    <div className="modal-overlay">
      <div
        className="modal-card"
        role="alertdialog"
        aria-modal="true"
        aria-label="Security key PIN"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-title-row">
          <span className="modal-title-icon" aria-hidden>
            <KeyRound size={18} />
          </span>
          <h3 className="modal-title">Security key PIN</h3>
        </div>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (pin) answer(pin);
          }}
        >
          <div className="modal-body">
            <p>
              {ask.kind === "wrong"
                ? `That PIN was wrong.${left}`
                : `Enter the PIN of your security key, then touch it when it blinks.${left}`}
            </p>
            <input
              type="password"
              autoFocus
              autoComplete="off"
              aria-label="Security key PIN"
              value={pin}
              onChange={(e) => setPin(e.target.value)}
            />
          </div>
          <div className="modal-actions">
            <button type="button" className="secondary" onClick={() => answer(null)}>
              Cancel
            </button>
            <button type="submit" disabled={!pin}>
              Continue
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
