import { describe, expect, it } from "vitest";
import { lastingTasks, syncTask } from "./backgroundTasks";

describe("background tasks", () => {
  it("lists what the pages keep running, with how far it got", () => {
    const tasks = lastingTasks(
      new Map<string, unknown>([
        ["copies.seeding", "disk"],
        ["copies.seed_progress", { objects_done: 1, objects_total: 4, bytes_done: 0, bytes_total: 0 }],
        ["verify.running", "deep"],
        ["verify.progress", ["Disk", 3, 10]],
      ]),
    );
    expect(tasks.map((t) => [t.id, t.detail])).toEqual([
      ["fill", "25%"],
      ["check", "30%"],
    ]);
  });

  it("leaves out values kept for something other than running work", () => {
    const tasks = lastingTasks(
      new Map<string, unknown>([
        ["updater", { phase: "available", version: "1.5.0" }],
        ["health.breaches", { kind: "done", report: {} }],
        ["backup.status", { kind: "ok", message: "Saved." }],
      ]),
    );
    expect(tasks).toEqual([]);
  });

  it("names the file a sync is moving", () => {
    const task = syncTask(
      {
        silo_id: "s",
        phase: "uploading",
        done: 1,
        total: 3,
        bytes_done: 50,
        bytes_total: 100,
        file_id: null,
        name: "a.zip",
      },
      false,
    );
    expect(task?.detail).toBe("a.zip, 50%");
    expect(syncTask(null, false)).toBeNull();
  });
});
