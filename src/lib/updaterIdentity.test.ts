import { describe, expect, it, vi } from "vitest";
import fs from "node:fs";
import path from "node:path";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

import { DOWNLOAD_PAGE } from "./updater";

// Every installed copy carries these values. Change any of them and those
// copies stop hearing about new versions (or lose their data folder and
// keyring entries), with nothing left to reach them through. Only with a
// transition plan: a release that knows both the old and the new value
// first, shipped and installed widely, before the old one goes.

/** Read by every copy from the bridge release on (docs/ARCHITECTURE.md, "Updates"), and
 * the download page they open. */
const LATEST_JSON = "https://silentsilo.com/latest.json";
const LATEST_JSON_FALLBACK =
  "https://raw.githubusercontent.com/silentsilo/website/main/public/latest.json";
const DOWNLOAD = "https://silentsilo.com/download/";

/** Read by 1.0.0 to 1.5.2, through the Tauri updater. Frozen on the bridge
 * release (docs/ARCHITECTURE.md, "Updates"); the release scripts still sign that
 * release with this key, read from tauri.conf.json. */
const PUBKEY =
  "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDc5RTUzMjIwQjc1Qzc5QjcKUldTM2VWeTNJRExsZWFuOUJyWldVYXhiVHlqK29LZm9jNGcrSUo2eGFBaFV3YlY4djhXbkRiWlAK";
const ENDPOINT = "https://releases.silentsilo.com/{{target}}/{{arch}}/{{current_version}}";

const IDENTIFIER = "com.silentsilo.desktop";
const PRODUCT_NAME = "SilentSilo";

type TauriConf = {
  productName: string;
  identifier: string;
  plugins: { updater: { pubkey: string; endpoints: string[] } };
};

const conf = JSON.parse(
  fs.readFileSync(path.resolve(__dirname, "../../src-tauri/tauri.conf.json"), "utf8"),
) as TauriConf;

const check = fs.readFileSync(
  path.resolve(__dirname, "../../src-tauri/src/update_check.rs"),
  "utf8",
);

/** A `pub const NAME: &str = "...";` from update_check.rs, however wrapped. */
function rustConst(name: string): string | undefined {
  return new RegExp(`pub const ${name}: &str =\\s*"([^"]+)";`).exec(check)?.[1];
}

describe("where every installed copy hears about a new version", () => {
  it("asks the website", () => {
    expect(rustConst("LATEST_JSON")).toBe(LATEST_JSON);
  });

  it("falls back to the website's repository on GitHub", () => {
    expect(rustConst("LATEST_JSON_FALLBACK")).toBe(LATEST_JSON_FALLBACK);
  });

  it("opens the same download page", () => {
    expect(DOWNLOAD_PAGE).toBe(DOWNLOAD);
  });
});

describe("what the copies before the bridge release rely on", () => {
  it("keeps the minisign public key the bridge release is signed with", () => {
    expect(conf.plugins.updater.pubkey).toBe(PUBKEY);
  });

  it("keeps the endpoint they ask", () => {
    expect(conf.plugins.updater.endpoints).toEqual([ENDPOINT]);
  });
});

describe("what names this install", () => {
  it("keeps the bundle identifier that names the data folder and keyring service", () => {
    expect(conf.identifier).toBe(IDENTIFIER);
  });

  it("keeps the product name", () => {
    expect(conf.productName).toBe(PRODUCT_NAME);
  });
});
