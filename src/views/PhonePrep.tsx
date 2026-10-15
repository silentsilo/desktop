import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft, Bluetooth, Check, Globe, Plus, QrCode, Smartphone, X } from "lucide-react";
import type { BluetoothState } from "../lib/types";
import { t, useLocale, type Key } from "../i18n";

type Props = {
  busy: boolean;
  /** Adding to a silo that has keys, one of which confirms first: only
   * the built-in authenticator, removable keys (a security key or a phone,
   * which look the same), or both. */
  adding?: "builtin" | "portable" | "both";
  /** Windows Hello, for the step that names it. */
  builtIn?: string;
  onContinue: () => void;
  onBack: () => void;
};

const BLUETOOTH_STATUS: Record<BluetoothState, Key> = {
  on: "start.phone_bt_status_on",
  off: "start.phone_bt_status_off",
  absent: "start.phone_bt_status_absent",
  unknown: "start.phone_bt_status_unknown",
};

/**
 * What using a phone as the key takes, said before Windows shows its QR
 * code. Windows checks the phone is nearby over Bluetooth, and without a
 * radio, or with it off, its own dialog fails without saying why. So the
 * radio is asked here first and its state shown beside what it takes.
 */
export function PhonePrep({ busy, adding, builtIn = "", onContinue, onBack }: Props) {
  useLocale();
  const [bluetooth, setBluetooth] = useState<BluetoothState | null>(null);

  const check = useCallback(() => {
    setBluetooth(null);
    invoke<BluetoothState>("bluetooth_state")
      .then(setBluetooth)
      // Could not ask: Windows says the rest.
      .catch(() => setBluetooth("unknown"));
  }, []);

  useEffect(() => {
    check();
  }, [check]);

  const blocked = bluetooth === "off" || bluetooth === "absent";
  const radio =
    bluetooth === null ? "is-checking" : blocked ? "is-bad" : bluetooth === "on" ? "is-good" : "";

  return (
    <div className="phone-prep">
      <div className="phone-prep-head">
        <span className="enroll-choice-icon" aria-hidden>
          <Smartphone size={20} />
        </span>
        <h3>{t("start.phone_title")}</h3>
      </div>

      <ol className="phone-prep-steps">
        {adding === "builtin" && <li>{t("start.phone_step_existing_builtin", { builtIn })}</li>}
        {adding === "portable" && <li>{t("start.phone_step_existing_key")}</li>}
        {adding === "both" && <li>{t("start.phone_step_existing", { builtIn })}</li>}
        <li>{adding ? t("start.phone_step_qr_new") : t("start.phone_step_qr")}</li>
        <li>{t("start.phone_step_confirm")}</li>
        <li>{t("start.phone_step_unlock")}</li>
      </ol>

      <ul className="phone-prep-needs">
        <li>
          <Smartphone size={15} aria-hidden />
          <span>{t("start.phone_need_phone")}</span>
        </li>
        <li className={radio}>
          <Bluetooth size={15} aria-hidden />
          <span>{t("start.phone_need_bt_here")}</span>
          <span className="phone-prep-status" role="status">
            {bluetooth === null ? (
              <span className="spinner" aria-hidden />
            ) : blocked ? (
              <X size={13} aria-hidden />
            ) : bluetooth === "on" ? (
              <Check size={13} aria-hidden />
            ) : null}
            {bluetooth === null ? t("start.phone_bt_status_checking") : t(BLUETOOTH_STATUS[bluetooth])}
          </span>
        </li>
        <li>
          <Globe size={15} aria-hidden />
          <span>{t("start.phone_need_online")}</span>
        </li>
      </ul>

      {bluetooth === "off" && <p className="error">{t("start.phone_bt_off")}</p>}
      {bluetooth === "absent" && <p className="error">{t("start.phone_bt_absent")}</p>}
      <p className="hint">{t("start.phone_where")}</p>

      <div className="actions">
        <button
          type="button"
          className="btn-primary"
          disabled={busy || bluetooth === null || blocked}
          onClick={onContinue}
        >
          {busy ? (
            <span className="spinner" aria-hidden />
          ) : adding ? (
            <Plus size={15} aria-hidden />
          ) : (
            <QrCode size={15} aria-hidden />
          )}
          {busy
            ? t("start.waiting")
            : adding
              ? t("start.phone_continue_add")
              : t("start.phone_continue")}
        </button>
        {blocked && (
          <button type="button" className="btn-secondary" disabled={busy} onClick={check}>
            {t("start.phone_check_again")}
          </button>
        )}
        <button type="button" className="btn-secondary" disabled={busy} onClick={onBack}>
          <ArrowLeft size={15} aria-hidden />
          {t("start.phone_back")}
        </button>
      </div>
    </div>
  );
}
