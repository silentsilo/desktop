import { useState } from "react";
import type { Os } from "../lib/platformStrings";
import { AlertTriangle, KeyRound, RefreshCw } from "lucide-react";
import { securityKeyDisplayName, usableHere } from "../lib/keyName";
import type { SecurityKeyInfo } from "../lib/types";

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

  const dropping = active.filter((k) => !keep.includes(k.credential_id));
  /// The same fallback the Security keys list uses, so one key carries one
  /// name across the page.
  const named = (k: SecurityKeyInfo) => securityKeyDisplayName(k, os);

  if (pending) {
    return (
      <div className="panel-section">
        <h3>
          <RefreshCw size={16} />
          Finish replacing the encryption key
        </h3>
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          Replacing the encryption key stopped before it finished. Syncing fails until you finish
          it, and it cannot be undone.
        </p>
        <p>
          Choose any enrolled key to finish with. Every other key stops opening the silo, so add
          them again afterwards.
        </p>
        <div className="actions">
          {active.filter(usableHere).map((k) => (
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
        Replace the encryption key
      </h3>
      <p>
        Removing a key is usually enough. On backup storage with versioning or object lock, a
        removed key can still open the silo. After you replace the encryption key, it cannot
        open anything added from then on.
      </p>
      <p className="hint">
        Your files are not re-encrypted or uploaded again, however large the silo. Only the file
        list and the keys are rewritten in each backup storage.
      </p>
      {archiveTargets > 0 && (
        <p className="hint is-error">
          A never-delete copy keeps the old key and stops receiving backups after the change.
          When it is done, remove that copy under Backup (what is stored there stays) and add a
          new never-delete copy.
        </p>
      )}

      <p>Tick the keys that should still open this silo. You will be asked to confirm with each one.</p>

      <ul className="key-list">
        {active.map((k) => (
          <li key={k.credential_id} className="key-list-item">
            <label className="key-choice">
              <input
                type="checkbox"
                checked={keep.includes(k.credential_id)}
                disabled={busy || !usableHere(k)}
                onChange={() => toggle(k.credential_id)}
              />
              <span>
                {named(k)}
                <span className="hint">
                  {!usableHere(k)
                    ? "From another device, so it cannot be kept from here. Add it again from that device afterwards."
                    : k.platform
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
          To use a key again, you would have to enrol it again.
        </p>
      )}

      {keep.length === 0 && (
        <p className="hint is-error" role="status">
          <AlertTriangle size={14} />
          Keep at least one key, or nothing would open this silo.
        </p>
      )}

      <p className="hint">
        Your recovery code changes too. The new one is shown once when this finishes, so write it
        down before closing the message.
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
          {busy ? "Working…" : "Replace the encryption key"}
        </button>
      </div>
    </div>
  );
}
