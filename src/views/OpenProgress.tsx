import { formatBytes } from "../lib/format";
import { t, useLocale } from "../i18n";

/** A file being made ready to open, as `open-progress` reports it. */
export type Opening = {
  fileId: string;
  name: string;
  phase: "preparing" | "downloading" | "decrypting" | "opening";
  done: number;
  total: number;
};

type Props = {
  opening: Opening;
  onCancel: () => void;
};

/** What the step is, in words, with the file's name. */
export function openingTitle(o: Opening): string {
  if (o.phase === "downloading") return t("start.opening_downloading", { name: o.name });
  if (o.phase === "decrypting") return t("start.opening_decrypting", { name: o.name });
  if (o.phase === "opening") return t("start.opening_opening", { name: o.name });
  return t("start.opening_preparing", { name: o.name });
}

/** How far the step is, 0 to 100, or null when it cannot be told. */
export function openingPercent(o: Opening): number | null {
  if (o.phase === "opening") return 100;
  if (o.phase === "preparing" || o.total <= 0) return null;
  return Math.min(99, Math.round((o.done / o.total) * 100));
}

/**
 * From the click until the file is handed to its application: a large file
 * takes seconds to fetch and decrypt, and with nothing on screen it looks
 * as if the click did nothing.
 */
export function OpenProgress({ opening, onCancel }: Props) {
  useLocale();
  const percent = openingPercent(opening);
  const detail =
    opening.phase === "downloading" && opening.total > 0
      ? t("start.of", { done: formatBytes(opening.done), total: formatBytes(opening.total) })
      : opening.phase === "decrypting" && percent !== null
        ? `${percent}%`
        : opening.phase === "opening"
          ? t("start.opening_handing")
          : "";

  return (
    <div className="toast open-progress" role="status" aria-live="polite">
      <div className="open-progress-body">
        <p className="open-progress-title" data-tooltip={opening.name}>
          {openingTitle(opening)}
        </p>
        <div
          className={`open-progress-bar${percent === null ? " is-waiting" : ""}`}
          role="progressbar"
          aria-label={openingTitle(opening)}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent ?? undefined}
        >
          <span style={percent === null ? undefined : { width: `${percent}%` }} />
        </div>
        {detail && <p className="open-progress-detail">{detail}</p>}
      </div>
      {opening.phase !== "opening" && (
        <button type="button" className="btn-secondary btn-sm open-progress-cancel" onClick={onCancel}>
          {t("common.cancel")}
        </button>
      )}
    </div>
  );
}
