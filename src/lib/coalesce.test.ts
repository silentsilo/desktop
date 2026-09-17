import { describe, expect, it } from "vitest";
import { coalesceLatest, coalesceRuns, type Scheduler } from "./coalesce";

/** A scheduler the test drives by hand, so no frames or timers are involved. */
function manualScheduler() {
  let queued: (() => void) | null = null;
  let cancelled = 0;
  const scheduler: Scheduler = {
    schedule: (run) => {
      queued = run;
      return 1;
    },
    cancel: () => {
      queued = null;
      cancelled += 1;
    },
  };
  return {
    scheduler,
    run: () => {
      const held = queued;
      queued = null;
      held?.();
    },
    get pending() {
      return queued !== null;
    },
    get cancelled() {
      return cancelled;
    },
  };
}

describe("coalesceLatest", () => {
  it("applies the last value of a burst, once", () => {
    const seen: number[] = [];
    const clock = manualScheduler();
    const { push } = coalesceLatest<number>((v) => seen.push(v), clock.scheduler);

    for (let i = 1; i <= 631; i += 1) push(i);
    expect(seen).toEqual([]);

    clock.run();
    expect(seen).toEqual([631]);
  });

  it("schedules once per burst rather than once per value", () => {
    let scheduled = 0;
    const clock = manualScheduler();
    const counting: Scheduler = {
      schedule: (run) => {
        scheduled += 1;
        return clock.scheduler.schedule(run);
      },
      cancel: clock.scheduler.cancel,
    };
    const { push } = coalesceLatest<number>(() => {}, counting);

    push(1);
    push(2);
    push(3);
    expect(scheduled).toBe(1);

    clock.run();
    push(4);
    expect(scheduled).toBe(2);
  });

  it("delivers a value that arrives after a flush", () => {
    const seen: number[] = [];
    const clock = manualScheduler();
    const { push } = coalesceLatest<number>((v) => seen.push(v), clock.scheduler);

    push(1);
    clock.run();
    push(2);
    clock.run();

    expect(seen).toEqual([1, 2]);
  });

  it("drops a pending value on stop, so nothing lands after teardown", () => {
    const seen: number[] = [];
    const clock = manualScheduler();
    const { push, stop } = coalesceLatest<number>((v) => seen.push(v), clock.scheduler);

    push(1);
    stop();
    clock.run();

    expect(seen).toEqual([]);
    expect(clock.cancelled).toBe(1);
  });
});

describe("coalesceRuns", () => {
  it("never runs two at once, and runs once more for what arrived meanwhile", async () => {
    let inFlight = 0;
    let maxInFlight = 0;
    let runs = 0;
    const waiting: Array<() => void> = [];
    const release = () => waiting.shift()?.();

    const call = coalesceRuns(async () => {
      runs += 1;
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      await new Promise<void>((resolve) => waiting.push(resolve));
      inFlight -= 1;
    });

    call();
    // Twenty reports while the first refresh is still in flight.
    for (let i = 0; i < 20; i += 1) call();
    expect(runs).toBe(1);

    release();
    await new Promise((r) => setTimeout(r, 0));
    expect(runs).toBe(2);
    expect(maxInFlight).toBe(1);

    release();
    await new Promise((r) => setTimeout(r, 0));
    // Nothing asked for anything during the trailing run.
    expect(runs).toBe(2);
  });

  it("keeps working after a run rejects", async () => {
    let runs = 0;
    const call = coalesceRuns(async () => {
      runs += 1;
      throw new Error("the backend said no");
    });

    call();
    await new Promise((r) => setTimeout(r, 0));
    call();
    await new Promise((r) => setTimeout(r, 0));

    expect(runs).toBe(2);
  });
});
