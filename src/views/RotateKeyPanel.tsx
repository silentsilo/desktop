import { useState } from "react";
import type { Os } from "../lib/platformStrings";
import { AlertTriangle, KeyRound, RefreshCw, Smartphone } from "lucide-react";
import { securityKeyDisplayName, usableHere } from "../lib/keyName";
import type { SecurityKeyInfo } from "../lib/types";
import { t, useLocale } from "../i18n";

type Props = {
  os: Os;
  busy: boolean;
  keys: SecurityKeyInfo[];
  /** Live instruction from the backend, naming the key to touch next. */
  progress: string | null;
  onRotate: (keep: string[]) => void;
  /** A key change that was started here and never finished. */
  pending: boolean;
  onResume: (credential: string) => void;
  /** Never-delete copies, which the change cannot reach. */
  archiveTargets: number;
};

/**
 * Changing the key everything in this silo is encrypted under.
 *
 * The reason it exists is that removing a security key is not always enough.
 * On storage that keeps what it is asked to delete, a versioned bucket or one
 * under object lock, the envelope that key unwraps stays readable and the key
 * goes on working. Rotating is the only real revocation there.
 *
 * Every cost is stated before the button rather than after. Choosing keys is
 * the operation, not a setting on it: what you tick keeps working and what
 * you leave stops, which is the whole point when one of them is in someone
 * else's pocket.
 */
export function RotateKeyPanel({
  os,
  busy,
  keys,
  progress,
  onRotate,
  pending,
  onResume,
  archiveTargets,
}: Props) {
  useLocale();
  // `fido_list_keys` returns the ones that still work, so there is nothing
  // to filter here.
  const active = keys;
  // Only keys this computer can touch can be kept: rotation re-wraps with
  // each one. A key from another device starts unticked and cannot be ticked.
  const keepable = (ids: string[]) =>
    ids.filter((id) => active.some((k) => k.credential_id === id && usableHere(k)));
  const [keep, setKeep] = useState<string[]>(() => keepable(active.map((k) => k.credential_id)));

  /// The list changes while this panel is on screen: enrolling a key in the
  /// section above updates `keys` but not this state, and the fresh key then
  /// rendered unticked, under a warning that it was about to stop working.
  /// A key that just appeared is ticked, because nobody adds a key in order
  /// to revoke it; one that disappeared is dropped from the selection.
  const idsNow = active.map((k) => k.credential_id);
  const [knownIds, setKnownIds] = useState(idsNow);
  if (knownIds.join("\n") !== idsNow.join("\n")) {
    const appeared = idsNow.filter((id) => !knownIds.includes(id));
    setKeep((prev) => [...prev.filter((id) => idsNow.includes(id)), ...keepable(appeared)]);
    setKnownIds(idsNow);
  }

  const toggle = (id: string) =>
    setKeep((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));

  // Keys on other devices are never a choice here: listed apart, without a
  // checkbox, and not part of the red warning, which is for what the user
  // chose to drop.
  const local = active.filter(usableHere);
  const elsewhere = active.filter((k) => !usableHere(k));
  const dropping = local.filter((k) => !keep.includes(k.credential_id));
  /// The same fallback the Security keys list uses, so one key carries one
  /// name across the page.
  const named = (k: SecurityKeyInfo) => securityKeyDisplayName(k, os);

  if (pending) {
    return (
      <div className="panel-section">
        <h3>
          <RefreshCw size={16} />
          {t("rotate.finish_title")}
        </h3>
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          {t("rotate.stopped")}
        </p>
        <p>{t("rotate.finish_body")}</p>
        <div className="actions">
          {active.filter(usableHere).map((k) => (
            <button
              className="btn-primary"
              key={k.credential_id}
              type="button"
              disabled={busy}
              onClick={() => onResume(k.credential_id)}
            >
              <KeyRound size={15} />
              {t("rotate.finish_with", { name: named(k) })}
            </button>
          ))}
        </div>
        {progress && (
          <p className="fido-live" role="status">
            {progress}
          </p>
        )}
      </div>
    );
  }

  return (
    <div className="panel-section">
      <h3>
        <RefreshCw size={16} />
        {t("rotate.title")}
      </h3>
      <p>{t("rotate.intro")}</p>
      <p className="hint">{t("rotate.no_reupload")}</p>
      {archiveTargets > 0 && <p className="hint is-error">{t("rotate.archive")}</p>}

      <p>{t("rotate.tick")}</p>

      <ul className="key-list">
        {local.map((k) => (
          <li key={k.credential_id} className="key-list-item">
            <label className="key-choice">
              <input
                type="checkbox"
                checked={keep.includes(k.credential_id)}
                disabled={busy}
                onChange={() => toggle(k.credential_id)}
              />
              <span>
                {named(k)}
                <span className="hint">
                  {k.platform ? t("rotate.built_in") : t("rotate.removable")}
                </span>
              </span>
            </label>
          </li>
        ))}
      </ul>

      {elsewhere.length > 0 && (
        <div className="rotate-elsewhere">
          <Smartphone size={16} aria-hidden />
          <p>
            {t("rotate.elsewhere", {
              count: elsewhere.length,
              names: elsewhere.map(named).join(", "),
            })}
          </p>
        </div>
      )}

      {dropping.length > 0 && (
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          {dropping.length === 1
            ? t("rotate.drop_one", { name: named(dropping[0]!) })
            : t("rotate.drop_many", {
                count: dropping.length,
                names: dropping.map(named).join(", "),
              })}{" "}
          {t("rotate.reenrol")}
        </p>
      )}

      {keep.length === 0 && (
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          {t("rotate.keep_one")}
        </p>
      )}

      <p className="hint">{t("rotate.code_changes")}</p>

      {progress && (
        <p className="fido-live" role="status">
          {progress}
        </p>
      )}

      <div className="actions">
        <button
          type="button"
          className="btn-danger"
          disabled={busy || keep.length === 0}
          onClick={() => onRotate(keep)}
        >
          <KeyRound size={15} />
          {busy ? t("rotate.working") : t("rotate.title")}
        </button>
      </div>
    </div>
  );
}
