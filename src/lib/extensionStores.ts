/**
 * Where the SilentSilo browser extension is listed. Each URL stays empty
 * until its listing exists, and a browser with an empty URL gets no link.
 * Brave installs from the Chrome Web Store. The extension ids the native
 * host lets in live beside the host, in
 * crates/silentsilo-browser-host/allowed-origins.json.
 */
const CHROME_WEB_STORE = "";
const EDGE_ADD_ONS = "";
const FIREFOX_ADD_ONS = "";

export interface StoreLink {
  browser: string;
  url: string;
}

export const EXTENSION_STORES: readonly StoreLink[] = [
  { browser: "Chrome", url: CHROME_WEB_STORE },
  { browser: "Edge", url: EDGE_ADD_ONS },
  { browser: "Brave", url: CHROME_WEB_STORE },
  { browser: "Firefox", url: FIREFOX_ADD_ONS },
];

/** The stores' own hosts: a listing URL anywhere else is not shown. */
const STORE_HOSTS = new Set([
  "chromewebstore.google.com",
  "microsoftedge.microsoft.com",
  "addons.mozilla.org",
]);

/** The links to show: filled in, https, on a store's own host. */
export function visibleStoreLinks(links: readonly StoreLink[]): StoreLink[] {
  return links.filter(({ url }) => {
    if (!url) return false;
    try {
      const parsed = new URL(url);
      return parsed.protocol === "https:" && STORE_HOSTS.has(parsed.hostname);
    } catch {
      return false;
    }
  });
}
