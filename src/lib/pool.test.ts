import { describe, expect, it } from "vitest";
import { mapPool } from "./pool";

describe("mapPool", () => {
  it("runs every item and preserves result order regardless of completion order", async () => {
    const results = await mapPool([30, 10, 20], 3, async (ms) => {
      await new Promise((r) => setTimeout(r, ms));
      return ms;
    });
    expect(results).toEqual([30, 10, 20]);
  });

  it("never runs more than `concurrency` items at once", async () => {
    let inFlight = 0;
    let maxInFlight = 0;
    await mapPool(Array.from({ length: 10 }, (_, i) => i), 3, async () => {
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await new Promise((r) => setTimeout(r, 5));
      inFlight -= 1;
    });
    expect(maxInFlight).toBeLessThanOrEqual(3);
  });

  it("stops picking up new items once isCancelled returns true", async () => {
    const processed: number[] = [];
    let cancelled = false;
    await mapPool(
      Array.from({ length: 20 }, (_, i) => i),
      1,
      async (i) => {
        processed.push(i);
        if (i === 2) cancelled = true;
      },
      () => cancelled,
    );
    // Item 2 triggers cancellation; item 3 must never start (concurrency 1
    // means "already in flight" is empty by the time cancellation is checked).
    expect(processed).toEqual([0, 1, 2]);
  });

  it("lets already in-flight items finish before stopping, with concurrency > 1", async () => {
    const started: number[] = [];
    let cancelled = false;
    // A shared signal so all 3 concurrent workers are already mid-flight
    // (past the isCancelled check) when cancellation happens, rather than
    // one worker's own synchronous work flipping the flag before its
    // siblings even get a turn to start.
    const cancelSignal = new Promise<void>((resolve) => {
      setTimeout(() => {
        cancelled = true;
        resolve();
      }, 5);
    });

    await mapPool(
      Array.from({ length: 10 }, (_, i) => i),
      3,
      async (i) => {
        started.push(i);
        await cancelSignal;
      },
      () => cancelled,
    );

    // The first 3 items are picked up together (concurrency 3) before any of
    // them observes cancellation — all 3 finish, nothing after.
    expect(started.length).toBe(3);
  });

  it("runs everything when isCancelled is never provided", async () => {
    const results = await mapPool([1, 2, 3], 2, async (n) => n * 2);
    expect(results).toEqual([2, 4, 6]);
  });
});
