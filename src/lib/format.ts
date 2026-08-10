import type { BreadcrumbSeg } from "./types";

export function formatBytes(n: number): string {
  if (n === 0) return "0 B";
  if (n < 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.floor(Math.log(n) / Math.log(k));
  const p = Math.min(i, sizes.length - 1);
  return `${parseFloat((n / Math.pow(k, p)).toFixed(1))} ${sizes[p]}`;
}

/**
 * The path as crumbs, rooted in the silo's own name.
 *
 * The root used to read "Silo", which named the concept rather than the
 * thing: with several silos open, every one of them claimed the same root
 * and the breadcrumb could not tell you which you were looking at.
 */
export function breadcrumbSegments(folderPath: string, rootLabel: string): BreadcrumbSeg[] {
  const segs: BreadcrumbSeg[] = [{ label: rootLabel, path: "/" }];
  const parts = folderPath.split("/").filter(Boolean);
  let acc = "";
  for (const part of parts) {
    acc += `/${part}`;
    segs.push({ label: part, path: acc });
  }
  return segs;
}

/**
 * A timestamp, with the year shown only when it isn't this one.
 *
 * Omitting it entirely made a file last touched in 2023 read as "Nov 15,
 * 00:13" — indistinguishable from one touched last week, which is the single
 * thing a modified column exists to tell you apart.
 */
export function formatDate(ts: number): string {
  if (!ts) return "—";
  const ms = ts > 1e12 ? ts : ts * 1000;
  const date = new Date(ms);
  const sameYear = date.getFullYear() === new Date().getFullYear();
  return date.toLocaleString(undefined, {
    year: sameYear ? undefined : "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  });
}

/**
 * A date without its time, in the same shape [`formatDate`] uses.
 *
 * For facts where the day is the story and the hour is noise: when a
 * recovery code was created, when a silo was last opened. The bare
 * `toLocaleDateString()` these places used before answered "1/21/2026" next
 * to a column of "Jan 21" timestamps, which read as two different products.
 */
export function formatDay(ts: number): string {
  if (!ts) return "—";
  const ms = ts > 1e12 ? ts : ts * 1000;
  const date = new Date(ms);
  const sameYear = date.getFullYear() === new Date().getFullYear();
  return date.toLocaleDateString(undefined, {
    year: sameYear ? undefined : "numeric",
    month: "short",
    day: "numeric",
  });
}
