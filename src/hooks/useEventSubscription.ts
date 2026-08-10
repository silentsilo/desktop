import { useEffect, type DependencyList } from "react";

type Unsubscribe = () => void;

/**
 * Starts a subscription and returns the function that ends it.
 *
 * The naive version of this — assign the unsubscribe function inside
 * `.then()`, call it on teardown — leaks a listener every time it re-runs.
 * Registration is asynchronous, so teardown can happen *before* the promise
 * resolves, at which point the variable it would read is still undefined:
 * nothing is unsubscribed, the promise later resolves against a closure
 * nobody will look at again, and that listener stays live forever.
 *
 * The symptom is duplicated work per event — one toast per leaked listener.
 *
 * Split out from the hook because the race is the whole substance here and
 * it is worth being able to test it without mounting a component.
 */
export function subscribeWithDisposal(subscribe: () => Promise<Unsubscribe>): Unsubscribe {
  let disposed = false;
  let unsubscribe: Unsubscribe | undefined;

  void subscribe().then((fn) => {
    // Arrived after teardown: end it immediately, since nothing else holds
    // a reference to it any more.
    if (disposed) {
      fn();
      return;
    }
    unsubscribe = fn;
  });

  return () => {
    disposed = true;
    // Cleared, so disposing twice does not unsubscribe twice. React never
    // does that, but this is a general-purpose function now and not every
    // subscription API tolerates it.
    const end = unsubscribe;
    unsubscribe = undefined;
    end?.();
  };
}

/**
 * Subscribes to a Tauri event for the lifetime of the effect.
 *
 * React's StrictMode mounts every effect twice in development precisely to
 * expose the leak described above, but it is not only a development problem:
 * any real re-subscription (a changed dependency) leaks the same way in a
 * release build.
 */
export function useEventSubscription(
  subscribe: () => Promise<Unsubscribe>,
  deps: DependencyList,
) {
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => subscribeWithDisposal(subscribe), deps);
}
