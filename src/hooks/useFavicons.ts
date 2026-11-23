import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

/** Icons fetched this run, by host: a `data:` URL, or null when the site has
 * none. In memory only: an `<img>` pointed at the site would leave the list
 * of sites in the WebView's cache on disk. */
const icons = new Map<string, string | null>();
const asked = new Set<string>();
/** Every mounted user of the hook, told when any icon arrives. */
const listeners = new Set<() => void>();

/** At most this many fetches at once, so a long list does not open a
 * connection per entry. */
const AT_ONCE = 6;
let running = 0;
const queue: Array<() => void> = [];

function next() {
  while (running < AT_ONCE && queue.length > 0) {
    running += 1;
    queue.shift()!();
  }
}

function ask(host: string) {
  if (asked.has(host)) return;
  asked.add(host);
  queue.push(() => {
    invoke<string | null>("favicon_fetch", { host })
      .catch(() => null)
      .then((icon) => {
        icons.set(host, icon ?? null);
        running -= 1;
        for (const listener of listeners) listener();
        next();
      });
  });
  next();
}

/** The icons of `hosts`, filled in as they arrive. A host missing from the
 * map is still on its way; null means there is no icon. */
export function useFavicons(hosts: readonly string[]): ReadonlyMap<string, string | null> {
  const key = [...new Set(hosts)].sort().join(" ");
  const [, setArrived] = useState(0);
  useEffect(() => {
    const arrived = () => setArrived((n) => n + 1);
    listeners.add(arrived);
    for (const host of key ? key.split(" ") : []) ask(host);
    return () => {
      listeners.delete(arrived);
    };
  }, [key]);
  return icons;
}
