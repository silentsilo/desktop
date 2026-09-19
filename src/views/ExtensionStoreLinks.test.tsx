import { describe, expect, it, vi } from "vitest";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { EXTENSION_STORES, type StoreLink, visibleStoreLinks } from "../lib/extensionStores";
import { ExtensionStoreLinks } from "./ExtensionStoreLinks";

const CHROME =
  "https://chromewebstore.google.com/detail/silentsilo/abcdefghijklmnopabcdefghijklmnop";

function links(chrome: string, edge = "", firefox = ""): StoreLink[] {
  return [
    { browser: "Chrome", url: chrome },
    { browser: "Edge", url: edge },
    { browser: "Brave", url: chrome },
    { browser: "Firefox", url: firefox },
  ];
}

/** The buttons in what the component returned, without a DOM. */
function buttons(node: ReactNode): ReactElement<{ onClick: () => void }>[] {
  if (!isValidElement<{ children?: ReactNode }>(node)) return [];
  if (node.type === "button") return [node as ReactElement<{ onClick: () => void }>];
  const children = node.props.children;
  return (Array.isArray(children) ? children : [children]).flatMap((c) => buttons(c));
}

describe("extension store links", () => {
  it("shows nothing while every URL is empty", () => {
    expect(renderToStaticMarkup(<ExtensionStoreLinks links={links("")} />)).toBe("");
    expect(ExtensionStoreLinks({ links: links("") })).toBeNull();
  });

  it("shows nothing today: no listing exists yet", () => {
    expect(EXTENSION_STORES.map((s) => s.url)).toEqual(["", "", "", ""]);
    expect(renderToStaticMarkup(<ExtensionStoreLinks links={EXTENSION_STORES} />)).toBe("");
  });

  it("shows a filled link, and Brave with Chrome's", () => {
    const html = renderToStaticMarkup(<ExtensionStoreLinks links={links(CHROME)} />);
    expect(html).toContain("Get it for Chrome");
    expect(html).toContain("Get it for Brave");
    expect(html).not.toContain("Edge");
    expect(html).not.toContain("Firefox");
  });

  it("opens the store page through the opener", () => {
    const open = vi.fn(() => Promise.resolve());
    const firefox = "https://addons.mozilla.org/firefox/addon/silentsilo/";
    const found = buttons(ExtensionStoreLinks({ links: links("", "", firefox), open }));
    expect(found).toHaveLength(1);
    found[0].props.onClick();
    expect(open).toHaveBeenCalledWith(firefox);
  });

  it("shows only https links on a store's own host", () => {
    for (const url of [
      "http://chromewebstore.google.com/detail/x",
      "https://example.com/detail/x",
      "https://chromewebstore.google.com.evil.example/x",
      "javascript:alert(1)",
      "file:///C:/x",
      "not a url",
    ]) {
      expect(visibleStoreLinks(links(url)), url).toEqual([]);
    }
    const edge = "https://microsoftedge.microsoft.com/addons/detail/x";
    expect(visibleStoreLinks(links("", edge))).toHaveLength(1);
  });
});
