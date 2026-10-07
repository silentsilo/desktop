import { dateLocale, t } from "../i18n";
import type { BreadcrumbSeg } from "./types";

/** The unit names, in the language in use: French writes octets. */
const units = () => [
  t("app.unit_b"),
  t("app.unit_kb"),
  t("app.unit_mb"),
  t("app.unit_gb"),
  t("app.unit_tb"),
];

export function formatBytes(n: number): string {
  const sizes = units();
  if (n <= 0) return `0 ${sizes[0]}`;
  const k = 1024;
  const i = Math.floor(Math.log(n) / Math.log(k));
  const p = Math.min(i, sizes.length - 1);
  // One decimal at most, with the language's own decimal mark and no
  // thousands separator, the shape "1.5 MB" always had.
  const value = new Intl.NumberFormat(dateLocale(), {
    maximumFractionDigits: 1,
    useGrouping: false,
  }).format(parseFloat((n / Math.pow(k, p)).toFixed(1)));
  return `${value} ${sizes[p]}`;
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
 * One date shape everywhere, day first: "15 Nov 2023, 14:00". British, like
 * the spelling; the system locale answered "Nov 15, 2023" next to it.
 */
// The language's own shape once one is chosen; English keeps the British
// day-first form.
const DATE_LOCALE = () => dateLocale();

/**
 * A timestamp, with the year shown only when it isn't this one.
 *
 * Omitting it entirely made a file last touched in 2023 read as "15 Nov,
 * 00:13", no different from one touched last week, which is the single
 * thing a modified column exists to tell you apart.
 */
export function formatDate(ts: number): string {
  if (!ts) return "-";
  const ms = ts > 1e12 ? ts : ts * 1000;
  const date = new Date(ms);
  const sameYear = date.getFullYear() === new Date().getFullYear();
  return date.toLocaleString(DATE_LOCALE(), {
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
  if (!ts) return "-";
  const ms = ts > 1e12 ? ts : ts * 1000;
  const date = new Date(ms);
  const sameYear = date.getFullYear() === new Date().getFullYear();
  return date.toLocaleDateString(DATE_LOCALE(), {
    year: sameYear ? undefined : "numeric",
    month: "short",
    day: "numeric",
  });
}

/** How long ago a Unix ms instant was, in the words a status line uses. */
export function formatAge(at: number): string {
  const seconds = Math.round((Date.now() - at) / 1000);
  if (seconds < 45) return t("app.age_just_now");
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return t("app.age_minutes", { count: minutes });
  const hours = Math.round(minutes / 60);
  if (hours < 24) return t("app.age_hours", { count: hours });
  // "49 hours ago" made the reader do arithmetic to learn it was the day
  // before yesterday.
  const days = Math.round(hours / 24);
  return days === 1 ? t("app.age_yesterday") : t("app.age_days", { count: days });
}
