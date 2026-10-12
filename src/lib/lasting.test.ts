import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  forgetEverySilo,
  forgetLasting,
  lastingGeneration,
  lastingKey,
  mountLasting,
  readLasting,
  resetLasting,
  unmountLasting,
  writeLasting,
} from "./lasting";

const key = lastingKey("silo-a", "backup.status");
const busy = (v: unknown) => v === "busy";

beforeEach(() => {
  resetLasting();
  vi.useFakeTimers();
});
afterEach(() => vi.useRealTimers());

describe("state that outlives its component", () => {
  it("keeps running work across leaving and coming back", () => {
    mountLasting(key);
    writeLasting(key, "idle", "busy");
    unmountLasting(key, busy);
    vi.runAllTimers();
    mountLasting(key);
    expect(readLasting(key, "idle")).toBe("busy");
  });

  it("shows an outcome that arrived while away once, then drops it", () => {
    mountLasting(key);
    writeLasting(key, "idle", "busy");
    unmountLasting(key, busy);
    vi.runAllTimers();
    writeLasting(key, "idle", "done");
    mountLasting(key);
    expect(readLasting(key, "idle")).toBe("done");
    unmountLasting(key, busy);
    vi.runAllTimers();
    expect(readLasting(key, "idle")).toBe("idle");
  });

  it("starts fresh after leaving with nothing running, like useState", () => {
    mountLasting(key);
    writeLasting(key, "idle", "done");
    unmountLasting(key, busy);
    vi.runAllTimers();
    expect(readLasting(key, "idle")).toBe("idle");
  });

  it("does not count a StrictMode remount as leaving", () => {
    writeLasting(key, "idle", "done");
    mountLasting(key);
    unmountLasting(key, busy);
    mountLasting(key);
    vi.runAllTimers();
    expect(readLasting(key, "idle")).toBe("done");
  });

  it("keeps each silo apart and forgets one on lock", () => {
    const other = lastingKey("silo-b", "backup.status");
    writeLasting(key, "idle", "busy");
    writeLasting(other, "idle", "busy");
    forgetLasting("silo-a");
    expect(readLasting(key, "idle")).toBe("idle");
    expect(readLasting(other, "idle")).toBe("busy");
  });
});

describe("a lock forgets what a silo kept", () => {
  it("every silo at once, the app's own state aside", () => {
    const other = lastingKey("silo-b", "pw.pending_import");
    const app = lastingKey("app", "updater");
    writeLasting(key, "idle", "done");
    writeLasting(other, null, "plaintext");
    writeLasting(app, "idle", "available");
    forgetEverySilo();
    expect(readLasting(key, "idle")).toBe("idle");
    expect(readLasting(other, null)).toBe(null);
    expect(readLasting(app, "idle")).toBe("available");
  });

  it("and work started before can tell", () => {
    const before = lastingGeneration("silo-a");
    forgetLasting("silo-b");
    expect(lastingGeneration("silo-a")).toBe(before);
    forgetLasting("silo-a");
    expect(lastingGeneration("silo-a")).not.toBe(before);
    const again = lastingGeneration("silo-a");
    forgetEverySilo();
    expect(lastingGeneration("silo-a")).not.toBe(again);
  });
});
