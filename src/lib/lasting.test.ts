import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  forgetLasting,
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
