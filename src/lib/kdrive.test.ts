import { describe, expect, it } from "vitest";
import { kdriveIdFrom, kdriveUrl, parseKdriveUrl } from "./kdrive";
import { EMPTY_STORE_DRAFT, missingStoreFields, storeDraftPayload } from "../views/StoreConfigForm";

describe("kDrive over WebDAV", () => {
  it("takes the ID bare or from a pasted kDrive address", () => {
    expect(kdriveIdFrom(" 123456 ")).toBe("123456");
    expect(kdriveIdFrom("https://kdrive.infomaniak.com/app/drive/654321/files")).toBe("654321");
  });

  it("builds the WebDAV address from the ID and the folder, and reads it back", () => {
    const url = kdriveUrl("123456", "Backups/Silo ă");
    expect(url).toBe("https://123456.connect.kdrive.infomaniak.com/Backups/Silo%20%C4%83");
    expect(parseKdriveUrl(url)).toEqual({ id: "123456", folder: "Backups/Silo ă" });
    expect(parseKdriveUrl("https://cloud.example.com/remote.php/dav")).toBeNull();
  });

  it("sends the built address and asks for the ID when it is missing", () => {
    const draft = {
      ...EMPTY_STORE_DRAFT,
      kind: "web-dav" as const,
      dav: { ...EMPTY_STORE_DRAFT.dav, preset: "kdrive" as const, username: "a@b.ch", password: "p" },
    };
    expect(missingStoreFields(draft, false)).toEqual(["kDrive ID"]);
    const ready = { ...draft, dav: { ...draft.dav, kdriveId: "42" } };
    expect(missingStoreFields(ready, false)).toEqual([]);
    expect(storeDraftPayload(ready)).toMatchObject({
      kind: "web-dav",
      url: "https://42.connect.kdrive.infomaniak.com/SilentSilo",
    });
  });
});
