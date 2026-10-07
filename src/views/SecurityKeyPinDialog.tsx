import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { KeyRound } from "lucide-react";
import { useEventSubscription } from "../hooks/useEventSubscription";
import { useModal } from "../hooks/useModal";
import { t, useLocale } from "../i18n";

/** What the key asked, as core sends it. */
type PinAsk = { kind: "enter" | "wrong"; retries: number | null };

/**
 * The security key's PIN, on Linux and macOS, where the app talks to the
 * key itself. Windows asks in its own dialog. The PIN goes to the key and
 * nowhere else; nothing here keeps it.
 */
export function SecurityKeyPinDialog() {
  useLocale();
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
  // Answered, cancelled or timed out in Rust: a PIN typed and not sent
  // goes too.
  useEventSubscription(
    () =>
      listen("fido-pin-done", () => {
        setAsk(null);
        setPin("");
      }),
    [],
  );

  const answer = (value: string | null) => {
    setAsk(null);
    setPin("");
    void invoke("fido_pin_answer", { pin: value }).catch(() => {});
  };
  // Asked while another dialog is open (a signature, a fill): this one is
  // on top, takes Escape and keeps Tab.
  const cardRef = useModal(() => answer(null), ask !== null);

  if (!ask) return null;

  const left =
    ask.retries === null
      ? ""
      : ask.retries === 1
        ? ` ${t("dlg.pin_last_try")}`
        : ` ${t("dlg.pin_tries_left", { count: ask.retries })}`;

  return (
    <div className="modal-overlay modal-overlay-top">
      <div
        ref={cardRef}
        className="modal-card"
        role="alertdialog"
        aria-modal="true"
        aria-label={t("dlg.pin_title")}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="modal-title-row">
          <span className="modal-title-icon" aria-hidden>
            <KeyRound size={18} />
          </span>
          <h3 className="modal-title">{t("dlg.pin_title")}</h3>
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
                ? `${t("dlg.pin_wrong")}${left}`
                : `${t("dlg.pin_enter")}${left}`}
            </p>
            <input
              type="password"
              autoFocus
              autoComplete="off"
              aria-label={t("dlg.pin_title")}
              value={pin}
              onChange={(e) => setPin(e.target.value)}
            />
          </div>
          <div className="modal-actions">
            <button type="button" className="secondary" onClick={() => answer(null)}>
              {t("common.cancel")}
            </button>
            <button type="submit" disabled={!pin}>
              {t("dlg.continue")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
