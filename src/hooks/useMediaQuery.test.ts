import { describe, expect, it, vi } from "vitest";
import { watchMediaQuery } from "./useMediaQuery";

/** A `MediaQueryList` whose state moves when the test says so. */
function fakeList(matches: boolean) {
  const listeners = new Set<() => void>();
  const list = {
    matches,
    addEventListener: (_type: string, fn: () => void) => {
      listeners.add(fn);
    },
    removeEventListener: (_type: string, fn: () => void) => {
      listeners.delete(fn);
    },
  };
  return {
    list,
    listenerCount: () => listeners.size,
    /** What the browser does when the viewport crosses the breakpoint. */
    set: (next: boolean) => {
      list.matches = next;
      listeners.forEach((fn) => fn());
    },
  };
}

describe("watchMediaQuery", () => {
  it("reports the current state on subscribe", () => {
    const onMatch = vi.fn();
    const { list } = fakeList(true);

    watchMediaQuery(list, onMatch);

    expect(onMatch).toHaveBeenCalledExactlyOnceWith(true);
  });

  it("reports every crossing of the breakpoint", () => {
    const onMatch = vi.fn();
    const { list, set } = fakeList(false);

    watchMediaQuery(list, onMatch);
    set(true);
    set(false);

    expect(onMatch.mock.calls.map(([m]) => m)).toEqual([false, true, false]);
  });

  it("stops reporting once disposed", () => {
    const onMatch = vi.fn();
    const { list, set, listenerCount } = fakeList(false);

    const dispose = watchMediaQuery(list, onMatch);
    dispose();
    set(true);

    expect(listenerCount()).toBe(0);
    expect(onMatch).toHaveBeenCalledExactlyOnceWith(false);
  });
});
