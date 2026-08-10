import { describe, expect, it, vi } from "vitest";
import { subscribeWithDisposal } from "./useEventSubscription";

/** A subscription whose registration resolves only when told to. */
function deferredSubscription() {
  const unsubscribe = vi.fn();
  let resolve!: (fn: () => void) => void;
  const promise = new Promise<() => void>((r) => {
    resolve = r;
  });
  return {
    unsubscribe,
    subscribe: () => promise,
    register: () => {
      resolve(unsubscribe);
      // Let the .then() callback run.
      return Promise.resolve();
    },
  };
}

describe("subscribeWithDisposal", () => {
  it("ends the subscription when disposed after it registered", async () => {
    const { subscribe, unsubscribe, register } = deferredSubscription();

    const dispose = subscribeWithDisposal(subscribe);
    await register();
    expect(unsubscribe).not.toHaveBeenCalled();

    dispose();
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("ends a subscription that registers after disposal", async () => {
    // The leak this exists to prevent: React tears the effect down before
    // the async registration resolves, so the listener would otherwise stay
    // live with nothing holding a reference to it — and every event after
    // that gets handled twice.
    const { subscribe, unsubscribe, register } = deferredSubscription();

    const dispose = subscribeWithDisposal(subscribe);
    dispose();
    expect(unsubscribe).not.toHaveBeenCalled();

    await register();
    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("does not end the same subscription twice", async () => {
    const { subscribe, unsubscribe, register } = deferredSubscription();

    const dispose = subscribeWithDisposal(subscribe);
    await register();
    dispose();
    dispose();

    expect(unsubscribe).toHaveBeenCalledTimes(1);
  });

  it("leaves exactly one listener alive across a StrictMode double-mount", async () => {
    // Two mounts, the first torn down immediately — which is what React does
    // in development. Both registrations resolve late, so only the surviving
    // one may still be subscribed.
    const first = deferredSubscription();
    const second = deferredSubscription();

    const disposeFirst = subscribeWithDisposal(first.subscribe);
    disposeFirst();
    const disposeSecond = subscribeWithDisposal(second.subscribe);

    await first.register();
    await second.register();

    expect(first.unsubscribe).toHaveBeenCalledTimes(1);
    expect(second.unsubscribe).not.toHaveBeenCalled();

    disposeSecond();
    expect(second.unsubscribe).toHaveBeenCalledTimes(1);
  });
});
