import { ExternalLink } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { type StoreLink, visibleStoreLinks } from "../lib/extensionStores";

/**
 * "Get it for ..." buttons for the stores that list the extension, opened
 * in the default browser. Nothing at all while no listing exists.
 */
export function ExtensionStoreLinks({
  links,
  open = openUrl,
}: {
  links: readonly StoreLink[];
  open?: (url: string) => Promise<void>;
}) {
  const shown = visibleStoreLinks(links);
  if (shown.length === 0) return null;
  return (
    <div className="actions">
      {shown.map(({ browser, url }) => (
        <button key={browser} type="button" className="secondary" onClick={() => void open(url)}>
          <ExternalLink size={14} />
          Get it for {browser}
        </button>
      ))}
    </div>
  );
}
