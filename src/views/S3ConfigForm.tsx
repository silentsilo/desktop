import type { S3Form, S3Preset } from "../lib/s3Presets";
import { S3_PRESETS } from "../lib/s3Presets";
import { plainHttpWarning, isPlainHttp } from "../lib/plainHttp";
import { t, useLocale } from "../i18n";

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
  useLocale();
  return (
    <>
      {/* Picking a provider is what fills the endpoint in, so the two read
          as one decision rather than two questions. */}
      <div className="s3-form-row">
        <label className="field">
          <span>{t("backup.s3_provider")}</span>
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
          <span>{t("backup.s3_endpoint")}</span>
          <input
            value={form.endpoint}
            onChange={(e) => set("endpoint", e.target.value)}
            placeholder="https://s3.example.com"
            spellCheck={false}
          />
          {isPlainHttp(form.endpoint) && <p className="hint">{plainHttpWarning()}</p>}
        </label>
      </div>

      <div className="s3-form-row">
        <label className="field">
          <span>{t("backup.s3_bucket")}</span>
          <input
            value={form.bucket}
            onChange={(e) => set("bucket", e.target.value)}
            spellCheck={false}
          />
        </label>
        <label className="field">
          <span>{t("backup.s3_region")}</span>
          <input
            value={form.region}
            onChange={(e) => set("region", e.target.value)}
            spellCheck={false}
          />
        </label>
      </div>

      <label className="field">
        <span>{t("backup.s3_prefix")}</span>
        <input
          value={form.prefix}
          onChange={(e) => set("prefix", e.target.value)}
          placeholder="silentsilo"
          spellCheck={false}
        />
        <p className="hint">{t("backup.s3_prefix_hint")}</p>
      </label>

      <div className="s3-form-row">
        <label className="field">
          <span>{t("backup.s3_access_key_id")}</span>
          <input
            value={form.accessKeyId}
            onChange={(e) => set("accessKeyId", e.target.value)}
            spellCheck={false}
            autoComplete="off"
          />
        </label>

        <label className="field">
          <span>{t("backup.s3_secret_key")}</span>
          <input
            type="password"
            value={form.secretAccessKey}
            onChange={(e) => set("secretAccessKey", e.target.value)}
            placeholder={connected ? t("backup.unchanged") : ""}
            autoComplete="off"
          />
          {connected && <p className="hint">{t("backup.s3_secret_hint")}</p>}
        </label>
      </div>

      <label className="s3-checkbox">
        <input
          type="checkbox"
          checked={form.pathStyle}
          onChange={(e) => set("pathStyle", e.target.checked)}
        />
        <span>
          {t("backup.s3_path_style")}
          <span className="hint">{t("backup.s3_path_style_hint")}</span>
        </span>
      </label>
    </>
  );
}
