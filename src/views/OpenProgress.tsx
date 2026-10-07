import { formatBytes } from "../lib/format";

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
  if (o.phase === "downloading") return `Downloading ${o.name}`;
  if (o.phase === "decrypting") return `Decrypting ${o.name}`;
  if (o.phase === "opening") return `Opening ${o.name}`;
  return `Getting ${o.name} ready`;
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
  const percent = openingPercent(opening);
  const detail =
    opening.phase === "downloading" && opening.total > 0
      ? `${formatBytes(opening.done)} of ${formatBytes(opening.total)}`
      : opening.phase === "decrypting" && percent !== null
        ? `${percent}%`
        : opening.phase === "opening"
          ? "Handing it to its application"
          : "";

  return (
    <div className="toast open-progress" role="status" aria-live="polite">
      <div className="open-progress-body">
        <p className="open-progress-title" title={opening.name}>
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
        <button type="button" className="secondary open-progress-cancel" onClick={onCancel}>
          Cancel
        </button>
      )}
    </div>
  );
}
