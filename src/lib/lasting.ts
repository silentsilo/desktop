import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useSyncExternalStore,
  type Dispatch,
  type SetStateAction,
} from "react";

/**
 * State that outlives the component showing it, for work that goes on after
 * the user looks elsewhere: a sync, a check, a seed, an import.
 *
 * Views and settings sections unmount when left. Their `useState` went with
 * them while the backend kept working, so coming back showed an idle button
 * for a sync still running, and the result of one that finished meanwhile
 * was never seen. Here the value lives in a module map: a promise settling
 * after the unmount still writes it, and the next mount reads it.
 *
 * A value is kept across an unmount only while `running` says so, plus one
 * outcome written while nobody was looking, which the next mount shows and
 * the one after drops. Everything else starts fresh, as `useState` would.
 */
const values = new Map<string, unknown>();
const listeners = new Map<string, Set<() => void>>();
const mounts = new Map<string, number>();
/** Outcomes written with no component mounted: shown once, then dropped. */
const unseen = new Set<string>();
/** What counts as running for each key, from the component that last used it. */
const runningOf = new Map<string, (value: unknown) => boolean>();
/** For the list of what runs: told about every change, whatever the key. */
const anyListeners = new Set<() => void>();
let version = 0;

/** Which silo the state belongs to. Empty before one is open. */
export const LastingScope = createContext("");

const SEP = "\u0000";
/** Not a silo id, so locking a silo never forgets it. */
const APP_SCOPE = "app";

export function lastingKey(scope: string, name: string): string {
  return scope + SEP + name;
}

function notify(key: string) {
  for (const listener of listeners.get(key) ?? []) listener();
  version += 1;
  for (const listener of anyListeners) listener();
}

export function readLasting<T>(key: string, initial: T): T {
  return values.has(key) ? (values.get(key) as T) : initial;
}

export function writeLasting<T>(key: string, initial: T, next: SetStateAction<T>) {
  const prev = readLasting(key, initial);
  const v = typeof next === "function" ? (next as (p: T) => T)(prev) : next;
  if (Object.is(v, prev)) return;
  values.set(key, v);
  if ((mounts.get(key) ?? 0) === 0) unseen.add(key);
  notify(key);
}

export function mountLasting(key: string, running?: (value: unknown) => boolean) {
  if (running) runningOf.set(key, running);
  mounts.set(key, (mounts.get(key) ?? 0) + 1);
  // On screen now, so an outcome from while it was away has been seen.
  unseen.delete(key);
}

/** Deferred: StrictMode unmounts and remounts at once in development, and
 * that must not count as leaving. */
export function unmountLasting(key: string, running: (value: unknown) => boolean) {
  mounts.set(key, Math.max(0, (mounts.get(key) ?? 1) - 1));
  setTimeout(() => {
    if ((mounts.get(key) ?? 0) > 0 || unseen.has(key)) return;
    if (values.has(key) && !running(values.get(key))) {
      values.delete(key);
      notify(key);
    }
  }, 0);
}

/** Drops everything kept for a silo, when it locks or is forgotten. */
export function forgetLasting(scope: string) {
  for (const key of [...values.keys()]) {
    if (key.startsWith(scope + SEP)) {
      values.delete(key);
      unseen.delete(key);
      notify(key);
    }
  }
}

/** For tests. */
export function resetLasting() {
  values.clear();
  unseen.clear();
  mounts.clear();
  runningOf.clear();
}

/** The values still running for a silo and for the app, by name. */
export function runningLasting(scope: string): Map<string, unknown> {
  const found = new Map<string, unknown>();
  for (const [key, value] of values) {
    const at = key.indexOf(SEP);
    const owner = key.slice(0, at);
    if (owner !== scope && owner !== APP_SCOPE) continue;
    if (runningOf.get(key)?.(value)) found.set(key.slice(at + 1), value);
  }
  return found;
}

/** [`runningLasting`], kept current. */
export function useRunningLasting(scope: string): Map<string, unknown> {
  const at = useSyncExternalStore(
    (listener) => {
      anyListeners.add(listener);
      return () => anyListeners.delete(listener);
    },
    () => version,
    () => version,
  );
  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => runningLasting(scope), [scope, at]);
}

/** `useState`, kept while `running` holds and for one unseen outcome.
 * Components using one `name` share the value. `app` keeps it for the app
 * rather than for the silo on screen. */
export function useLasting<T>(
  name: string,
  initial: T,
  running: (value: T) => boolean,
  app = false,
): [T, Dispatch<SetStateAction<T>>] {
  const silo = useContext(LastingScope);
  const key = lastingKey(app ? APP_SCOPE : silo, name);
  const initialRef = useRef(initial);
  const runningRef = useRef(running);
  runningRef.current = running;

  const subscribe = useCallback(
    (listener: () => void) => {
      const set = listeners.get(key) ?? new Set();
      listeners.set(key, set);
      set.add(listener);
      return () => {
        set.delete(listener);
      };
    },
    [key],
  );
  const read = () => readLasting(key, initialRef.current);
  const value = useSyncExternalStore(subscribe, read, read);

  const set = useCallback(
    (next: SetStateAction<T>) => writeLasting(key, initialRef.current, next),
    [key],
  );

  useEffect(() => {
    const running = (v: unknown) => runningRef.current(v as T);
    mountLasting(key, running);
    return () => unmountLasting(key, running);
  }, [key]);

  return [value, set];
}
