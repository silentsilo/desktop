import { useCallback, useState } from "react";

/**
 * What kind of work is in flight, so one kind does not disable another.
 *
 * There used to be a single `busy` flag for the whole app, which meant
 * stepping into a folder greyed out every button on screen, including the
 * ones for work that had nothing to do with it. That is most of what made
 * the app feel heavy: the freeze was not the operation taking long, it was
 * everything else refusing to respond while it did.
 *
 * The kinds are deliberately coarse. The question each one answers is "would
 * starting this while that runs produce a wrong result or a confusing one",
 * not "is this a different function".
 */
export type JobKind =
  /** Moving around: opening a folder, back, forward, up, refreshing. */
  | "navigate"
  /** Moving bytes: import, export, opening a file, fetching content. */
  | "transfer"
  /** Changing one entry: rename, trash, restore, favourite, new folder. */
  | "entry"
  /** Credentials: saving, deleting or importing password entries. */
  | "entries"
  /** The silo itself: creating, adding, forgetting, locking, unlocking. */
  | "silo"
  /** Security keys and recovery codes. */
  | "keys";

export function useJobs() {
  // Counted rather than a boolean per kind: two imports at once must not
  // have the first one finishing declare the second one done.
  const [counts, setCounts] = useState<Partial<Record<JobKind, number>>>({});

  const begin = useCallback((kind: JobKind) => {
    setCounts((current) => ({ ...current, [kind]: (current[kind] ?? 0) + 1 }));
  }, []);

  const end = useCallback((kind: JobKind) => {
    setCounts((current) => ({
      ...current,
      [kind]: Math.max(0, (current[kind] ?? 1) - 1),
    }));
  }, []);

  const busy = useCallback(
    (...kinds: JobKind[]) => kinds.some((kind) => (counts[kind] ?? 0) > 0),
    [counts],
  );

  return { begin, end, busy };
}
