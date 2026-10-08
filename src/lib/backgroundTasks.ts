import { t } from "../i18n";
import type { SeedProgress, SyncProgress, View } from "./types";

/** Where a task is followed: a view, or a Settings section by id. */
export type TaskPlace = { view: Exclude<View, "settings"> } | { section: string };

export type BackgroundTask = {
  id: string;
  label: string;
  /** How far, when the work says: "45%" or "3 of 10". */
  detail: string | null;
  place: TaskPlace;
};

function percent(done: number, total: number): string | null {
  if (total <= 0) return null;
  return `${Math.min(100, Math.floor((done / total) * 100))}%`;
}

/**
 * What the pages that keep their work in lib/lasting have running, by the
 * names they keep it under. Each value is checked here as well: a lasting
 * value can be kept for something other than running work (the updater
 * keeps a found update, Health keeps the breach answer).
 */
export function lastingTasks(running: Map<string, unknown>): BackgroundTask[] {
  const tasks: BackgroundTask[] = [];
  const has = (name: string) => running.has(name);

  // Sync now, or a test, save or disconnect, started on the Backup page; its
  // own message names it. A pass the app runs by itself comes from syncTask.
  const backup = running.get("backup.status") as { kind: string; message?: string } | undefined;
  if (backup?.kind === "busy" && backup.message) {
    tasks.push({
      id: "backup",
      label: backup.message.replace(/…$/, ""),
      detail: null,
      place: { section: "backup" },
    });
  }
  if (has("copies.seeding")) {
    const p = running.get("copies.seed_progress") as SeedProgress | undefined;
    tasks.push({
      id: "fill",
      label: t("nav.task_fill"),
      detail: p
        ? p.bytes_total > 0
          ? percent(p.bytes_done, p.bytes_total)
          : percent(p.objects_done, p.objects_total)
        : null,
      place: { section: "backup" },
    });
  } else if (has("copies.working")) {
    tasks.push({ id: "copies", label: t("nav.task_copies"), detail: null, place: { section: "backup" } });
  }
  if (has("verify.running")) {
    const p = running.get("verify.progress") as [string, number, number] | undefined;
    tasks.push({
      id: "check",
      label: t("nav.task_check"),
      detail: p ? percent(p[1], p[2]) : null,
      place: { section: "verify" },
    });
  }
  if (has("restore.running")) {
    const p = running.get("restore.progress") as [number, number] | undefined;
    tasks.push({
      id: "restore",
      label: t("nav.task_restore_test"),
      detail: p ? percent(p[0], p[1]) : null,
      place: { section: "verify" },
    });
  }
  if ((running.get("health.breaches") as { kind: string } | undefined)?.kind === "busy") {
    tasks.push({ id: "breaches", label: t("nav.task_breaches"), detail: null, place: { view: "health" } });
  }
  if (has("pw.transfer_busy")) {
    tasks.push({ id: "passwords", label: t("nav.task_passwords"), detail: null, place: { view: "passwords" } });
  }
  if (has("protected.busy")) {
    tasks.push({ id: "scan", label: t("nav.task_scan"), detail: null, place: { section: "protected" } });
  }
  const update = running.get("updater") as
    | { phase: string; downloaded?: number; contentLength?: number | null }
    | undefined;
  if (update?.phase === "installing") {
    tasks.push({
      id: "update",
      label: t("nav.task_update"),
      detail: update.contentLength ? percent(update.downloaded ?? 0, update.contentLength) : null,
      place: { section: "updates" },
    });
  }
  return tasks;
}

/** A sync pass in flight, from its own progress. */
export function syncTask(progress: SyncProgress | null, syncing: boolean): BackgroundTask | null {
  if (!progress && !syncing) return null;
  const detail = !progress
    ? null
    : progress.bytes_total > 0
      ? percent(progress.bytes_done, progress.bytes_total)
      : percent(progress.done, progress.total);
  return {
    id: "sync",
    label: t("nav.task_sync"),
    detail: progress?.name && detail ? `${progress.name}, ${detail}` : detail,
    place: { section: "backup" },
  };
}
