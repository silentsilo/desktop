import { describe, expect, it, vi } from "vitest";
import fs from "node:fs";
import path from "node:path";

vi.mock("@tauri-apps/plugin-updater", () => ({ check: vi.fn() }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { RELEASES } from "./updater";

// Every installed copy carries these values. Change any of them and those
// copies stop updating (or lose their data folder and keyring entries), with
// no way to reach them through the updater. Only with a transition plan:
// a release that trusts both the old and the new value first, shipped and
// installed widely, before the old one goes.

const PUBKEY =
  "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDc5RTUzMjIwQjc1Qzc5QjcKUldTM2VWeTNJRExsZWFuOUJyWldVYXhiVHlqK29LZm9jNGcrSUo2eGFBaFV3YlY4djhXbkRiWlAK";
const ENDPOINT = "https://releases.silentsilo.com/{{target}}/{{arch}}/{{current_version}}";
const RELEASES_PREFIX = "https://github.com/silentsilo/desktop/releases/download/";
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

describe("the updater identity every installed copy relies on", () => {
  it("keeps the minisign public key", () => {
    expect(conf.plugins.updater.pubkey).toBe(PUBKEY);
  });

  it("asks the same endpoint and no other", () => {
    expect(conf.plugins.updater.endpoints).toEqual([ENDPOINT]);
  });

  it("takes updates only from the same releases", () => {
    expect(RELEASES).toBe(RELEASES_PREFIX);
  });

  it("keeps the bundle identifier that names the data folder and keyring service", () => {
    expect(conf.identifier).toBe(IDENTIFIER);
  });

  it("keeps the product name", () => {
    expect(conf.productName).toBe(PRODUCT_NAME);
  });
});
