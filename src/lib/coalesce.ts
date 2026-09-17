/**
 * Two ways of surviving a burst of backend events.
 *
 * A long job reports as it goes: filling a second copy emits one
 * `seed-progress` per object, a pass emits `sync-progress` as it works and
 * `sync-report` when it ends. Each event arrives in its own task, so React
 * cannot batch them: handled one for one, a copy of a few hundred objects is
 * a few hundred renders of the panel, and every screen the report touches
 * re-renders again on top. The window stays alive through that, but the
 * frames it spends redrawing counts nobody reads are frames it is not
 * spending on the Stop button.
 *
 * Neither of these drops the last value. A progress line that stops one
 * object short, or a list left showing a backlog that has already cleared,
 * is the bug this would otherwise introduce.
 */

/** Where a coalesced update is run. Injected so the tests need no frames. */
export type Scheduler = {
  schedule: (run: () => void) => number;
  cancel: (handle: number) => void;
};

/**
 * One animation frame. Falls back to a timer where there is no frame to wait
 * for, which is any non-browser environment and a hidden window.
 */
export const frameScheduler: Scheduler = {
  schedule: (run) =>
    typeof requestAnimationFrame === "function"
      ? requestAnimationFrame(() => run())
      : (setTimeout(run, 16) as unknown as number),
  cancel: (handle) => {
    if (typeof cancelAnimationFrame === "function") cancelAnimationFrame(handle);
    else clearTimeout(handle);
  },
};

/**
 * Applies only the most recent value, at most once per scheduled turn.
 *
 * For a counter going up: every value but the last is already stale by the
 * time it could be drawn, so the ones in between are worth nothing and cost
 * a render each.
 */
export function coalesceLatest<T>(
  apply: (value: T) => void,
  scheduler: Scheduler = frameScheduler,
): { push: (value: T) => void; stop: () => void } {
  let pending: { value: T } | null = null;
  let handle: number | null = null;

  const flush = () => {
    handle = null;
    const held = pending;
    pending = null;
    if (held) apply(held.value);
  };

  return {
    push: (value: T) => {
      pending = { value };
      if (handle === null) handle = scheduler.schedule(flush);
    },
    // Called on teardown: a frame that fires after the component is gone
    // would set state on something nobody is looking at.
    stop: () => {
      if (handle !== null) scheduler.cancel(handle);
      handle = null;
      pending = null;
    },
  };
}

/**
 * Runs `work` one at a time, collapsing everything asked for while it is in
 * flight into a single further run.
 *
 * For a refresh that costs a round trip to the backend. A burst of reports
 * would otherwise queue one call per report, each reading the same state a
 * moment apart, and the last of them is the only answer anyone wanted. The
 * trailing run is what keeps the panel truthful: something changed after
 * this run read its state, so it is read once more.
 *
 * No interval to tune: the work's own duration is the rate limit.
 */
export function coalesceRuns(work: () => Promise<void>): () => void {
  let running = false;
  let askedAgain = false;

  const start = () => {
    running = true;
    void work()
      .catch(() => {
        // The caller reports its own failures; swallowing here only stops an
        // unhandled rejection from ending the chain and wedging the refresh.
      })
      .finally(() => {
        running = false;
        if (askedAgain) {
          askedAgain = false;
          start();
        }
      });
  };

  return () => {
    if (running) askedAgain = true;
    else start();
  };
}
