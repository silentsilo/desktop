/**
 * Run async work over `items` with at most `concurrency` in flight.
 * If `isCancelled` starts returning true, no further items are picked up —
 * whatever's already in flight (up to `concurrency` of them) still finishes.
 */
export async function mapPool<T, R>(
  items: T[],
  concurrency: number,
  worker: (item: T, index: number) => Promise<R>,
  isCancelled?: () => boolean,
): Promise<R[]> {
  const limit = Math.max(1, Math.min(concurrency, items.length || 1));
  const results: R[] = new Array(items.length);
  let next = 0;

  const run = async () => {
    while (true) {
      if (isCancelled?.()) return;
      const i = next++;
      if (i >= items.length) return;
      results[i] = await worker(items[i]!, i);
    }
  };

  await Promise.all(Array.from({ length: limit }, () => run()));
  return results;
}

/** Upload concurrency: 2–4 based on CPU cores. */
export function uploadConcurrency(): number {
  const cores =
    typeof navigator !== "undefined" && navigator.hardwareConcurrency
      ? navigator.hardwareConcurrency
      : 4;
  return Math.min(4, Math.max(2, Math.floor(cores / 2) || 2));
}
