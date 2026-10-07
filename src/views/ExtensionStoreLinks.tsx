import { ExternalLink } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { type StoreLink, visibleStoreLinks } from "../lib/extensionStores";
import { t } from "../i18n";

/**
 * "Get it for ..." buttons for the stores that list the extension, opened
 * in the default browser. Nothing at all while no listing exists.
 * `onChosen` hears the click first: someone getting the extension wants it
 * to reach the app, so Settings turns the connection on then.
 * No useLocale here: the tests call it as a plain function, and its
 * parent re-renders it when the language changes.
 */
export function ExtensionStoreLinks({
  links,
  open = openUrl,
  onChosen,
}: {
  links: readonly StoreLink[];
  open?: (url: string) => Promise<void>;
  onChosen?: () => void;
}) {
  const shown = visibleStoreLinks(links);
  if (shown.length === 0) return null;
  return (
    <div className="actions">
      {shown.map(({ browser, url }) => (
        <button
          key={browser}
          type="button"
          className="secondary"
          onClick={() => {
            onChosen?.();
            void open(url);
          }}
        >
          <ExternalLink size={14} />
          {t("set.get_for", { browser })}
        </button>
      ))}
    </div>
  );
}
