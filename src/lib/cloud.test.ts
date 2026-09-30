import { describe, expect, it } from "vitest";
import { cloudFolderProblem } from "./cloud";
import { formatAppError } from "./errors";
import {
  EMPTY_STORE_DRAFT,
  missingStoreFields,
  storeDraftPayload,
  type StoreDraft,
} from "../views/StoreConfigForm";

function onedrive(patch: Partial<StoreDraft["cloud"]["onedrive"]>): StoreDraft {
  return {
    ...EMPTY_STORE_DRAFT,
    kind: "onedrive",
    cloud: {
      ...EMPTY_STORE_DRAFT.cloud,
      onedrive: { ...EMPTY_STORE_DRAFT.cloud.onedrive, ...patch },
    },
  };
}

describe("cloud storage in the form", () => {
  it("sends the sign-in id and the folder, never the account", () => {
    const payload = storeDraftPayload(
      onedrive({ signIn: "id-1", account: "ana@outlook.com", folder: " Silo " }),
    );
    expect(payload).toEqual({ kind: "onedrive", signIn: "id-1", folder: "Silo" });
  });

  it("starts on a folder name that says nothing about the silo", () => {
    expect(EMPTY_STORE_DRAFT.cloud["google-drive"].folder).toBe("Silo");
  });

  it("asks for a sign-in first, unless the copy is already connected", () => {
    expect(missingStoreFields(onedrive({}), false)).toEqual(["a OneDrive sign-in"]);
    expect(missingStoreFields(onedrive({ account: "ana@outlook.com" }), true)).toEqual([]);
    expect(missingStoreFields(onedrive({ signIn: "id-1", folder: " " }), false)).toEqual([
      "folder name",
    ]);
  });

  it("refuses a folder name that is a path or that OneDrive would refuse", () => {
    for (const bad of ["", "a/b", "a\\b", "..", ".hidden", "end.", "a:b", "a|b", 'a"b']) {
      expect(cloudFolderProblem(bad), bad).not.toBeNull();
    }
    expect(cloudFolderProblem("Siloz personal ăîș")).toBeNull();
  });
});

describe("cloud errors", () => {
  it("names the provider when a sign-in stopped working", () => {
    expect(formatAppError("storage rejected the request: Sign in to Dropbox again")).toContain(
      "Use Sign in again",
    );
  });

  it("says which provider is full", () => {
    expect(formatAppError("storage error: Google Drive is full")).toBe(
      "Google Drive is full. Free some space there, or keep this silo somewhere else too.",
    );
  });

  it("does not blame a security key for a sign-in cancelled in the browser", () => {
    expect(formatAppError("storage rejected the request: the sign-in was cancelled")).toBe(
      "The sign-in was cancelled in the browser.",
    );
  });

  it("passes core's own sentences through without the storage prefix", () => {
    expect(
      formatAppError(
        "storage rejected the request: OneDrive for work or school accounts is not supported yet",
      ),
    ).toBe("OneDrive for work or school accounts is not supported yet");
    expect(
      formatAppError(
        "storage rejected the request: That is a different Dropbox account. Sign in with the one this storage uses.",
      ),
    ).toBe("That is a different Dropbox account. Sign in with the one this storage uses.");
  });
});
