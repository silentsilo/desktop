import { useState } from "react";
import { AlertTriangle, KeyRound, RefreshCw } from "lucide-react";
import { securityKeyDisplayName } from "../lib/keyName";
import type { SecurityKeyInfo } from "../lib/types";

type Props = {
  busy: boolean;
  keys: SecurityKeyInfo[];
  /** Live instruction from the backend, naming the key to touch next. */
  progress: string | null;
  onRotate: (keep: string[]) => void;
  /** A key change that was started here and never finished. */
  pending: boolean;
  onResume: (credential: string) => void;
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
export function RotateKeyPanel({ busy, keys, progress, onRotate, pending, onResume }: Props) {
  // `fido_list_keys` returns the ones that still work, so there is nothing
  // to filter here.
  const active = keys;
  const [keep, setKeep] = useState<string[]>(() => active.map((k) => k.credential_id));

  const toggle = (id: string) =>
    setKeep((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));

  const dropping = active.filter((k) => !keep.includes(k.credential_id));
  /// The same fallback the Security keys list uses, so one key carries one
  /// name across the page.
  const named = (k: SecurityKeyInfo) => securityKeyDisplayName(k);

  if (pending) {
    return (
      <div className="panel-section">
        <h3>
          <RefreshCw size={16} />
          Finish changing the encryption key
        </h3>
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          A key change was started here and stopped before it finished. Your storage is part-way
          converted, so syncing will keep failing until this is done. Finishing it is the only way
          out: once objects have been re-sealed, going back is not possible.
        </p>
        <p>
          Choose a key to finish with. Any enrolled key works, including one that was not used to
          start this. Every other key stops opening the silo, so add them again afterwards.
        </p>
        <div className="actions">
          {active.map((k) => (
            <button
              key={k.credential_id}
              type="button"
              disabled={busy}
              onClick={() => onResume(k.credential_id)}
            >
              <KeyRound size={15} />
              Finish with {named(k)}
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
        Change the silo's encryption key
      </h3>
      <p>
        Removing a security key is usually enough. It is not enough on storage that refuses to
        delete: a versioned bucket, or one under object lock, keeps the removed key's envelope
        readable and that key goes on opening the silo. Changing the encryption key is the only
        thing that really stops it.
      </p>
      <p className="hint">
        Your files are not re-encrypted and nothing is re-uploaded, however large the silo. Only
        the keys change, which takes seconds.
      </p>

      <p>Tick the keys that should still open this silo. You will be asked to touch each one.</p>

      <ul className="key-list">
        {active.map((k) => (
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
                  {k.platform
                    ? "Built into this computer"
                    : "Removable key, needs to be plugged in"}
                </span>
              </span>
            </label>
          </li>
        ))}
      </ul>

      {dropping.length > 0 && (
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          {dropping.length === 1
            ? `“${named(dropping[0]!)}” will stop opening this silo.`
            : `${dropping.length} keys will stop opening this silo: ${dropping
                .map(named)
                .join(", ")}.`}{" "}
          That is what this is for, and it cannot be undone from here. They would have to be
          enrolled again.
        </p>
      )}

      {keep.length === 0 && (
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          Keep at least one key, or nothing would open this silo.
        </p>
      )}

      <p className="hint">
        Your recovery code changes too, because the old one unwraps the old key. A new one is
        shown once when this finishes. Write it down before closing the message.
      </p>

      {progress && (
        <p className="fido-live" role="status">
          {progress}
        </p>
      )}

      <div className="actions">
        <button
          type="button"
          className="danger"
          disabled={busy || keep.length === 0}
          onClick={() => onRotate(keep)}
        >
          <KeyRound size={15} />
          {busy ? "Working…" : "Change the encryption key"}
        </button>
      </div>
    </div>
  );
}
