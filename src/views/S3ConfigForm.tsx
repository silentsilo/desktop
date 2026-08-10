import type { S3Form, S3Preset } from "../lib/s3Presets";
import { S3_PRESETS } from "../lib/s3Presets";
import { PLAIN_HTTP_WARNING, isPlainHttp } from "../lib/plainHttp";

type Props = {
  form: S3Form;
  set: <K extends keyof S3Form>(key: K, value: S3Form[K]) => void;
  preset: S3Preset;
  choosePreset: (id: string) => void;
  /** Whether a config is already stored, which changes what the secret field means. */
  connected: boolean;
};

/**
 * The bucket credential fields on their own.
 *
 * Shared by the settings panel and by first-run joining, because a device
 * that is about to join a silo has to describe the same bucket in the same
 * way — and the two drifting apart would mean one of them silently accepting
 * a config the other rejects.
 */
export function S3ConfigForm({ form, set, preset, choosePreset, connected }: Props) {
  return (
    <>
      <label className="field">
        <span>Provider</span>
        <select value={preset.id} onChange={(e) => choosePreset(e.target.value)}>
          {S3_PRESETS.map((p) => (
            <option key={p.id} value={p.id}>
              {p.label}
            </option>
          ))}
        </select>
        <p className="hint">{preset.hint}</p>
      </label>

      <label className="field">
        <span>Endpoint</span>
        <input
          value={form.endpoint}
          onChange={(e) => set("endpoint", e.target.value)}
          placeholder="https://s3.example.com"
          spellCheck={false}
        />
        {isPlainHttp(form.endpoint) && <p className="hint">{PLAIN_HTTP_WARNING}</p>}
      </label>

      <div className="s3-form-row">
        <label className="field">
          <span>Bucket</span>
          <input
            value={form.bucket}
            onChange={(e) => set("bucket", e.target.value)}
            spellCheck={false}
          />
        </label>
        <label className="field">
          <span>Region</span>
          <input
            value={form.region}
            onChange={(e) => set("region", e.target.value)}
            spellCheck={false}
          />
        </label>
      </div>

      <label className="field">
        <span>Folder inside the bucket</span>
        <input
          value={form.prefix}
          onChange={(e) => set("prefix", e.target.value)}
          placeholder="silentsilo"
          spellCheck={false}
        />
        <p className="hint">Leave blank to use the bucket root.</p>
      </label>

      <label className="field">
        <span>Access key ID</span>
        <input
          value={form.accessKeyId}
          onChange={(e) => set("accessKeyId", e.target.value)}
          spellCheck={false}
          autoComplete="off"
        />
      </label>

      <label className="field">
        <span>Secret access key</span>
        <input
          type="password"
          value={form.secretAccessKey}
          onChange={(e) => set("secretAccessKey", e.target.value)}
          placeholder={connected ? "unchanged" : ""}
          autoComplete="off"
        />
        {connected && <p className="hint">Leave blank to keep the stored key.</p>}
      </label>

      <label className="s3-checkbox">
        <input
          type="checkbox"
          checked={form.pathStyle}
          onChange={(e) => set("pathStyle", e.target.checked)}
        />
        <span>
          Path-style addressing
          <span className="hint">
            Required by MinIO and some self-hosted providers. If uploads fail with a host or DNS
            error, try flipping this.
          </span>
        </span>
      </label>
    </>
  );
}
