import { describe, expect, it } from "vitest";
import { describeRestoreDifference } from "./restoreDiff";

describe("describeRestoreDifference", () => {
  it("names a file with its path and size", () => {
    expect(
      describeRestoreDifference(
        "only here: file /Invoices::March.pdf deleted=false fav=0 size=88123 hash=ab mime=application/pdf",
      ),
    ).toBe("Only in this silo: the file /Invoices/March.pdf (86.1 KB).");
  });

  it("joins a root file without doubling the slash", () => {
    expect(
      describeRestoreDifference(
        "only in the restore: file /::notes.txt deleted=false fav=1 size=2048 hash= mime=",
      ),
    ).toBe("Only in the restore: the file /notes.txt (2 KB).");
  });

  it("says when the file is in the trash", () => {
    expect(
      describeRestoreDifference(
        "only here: file /::old.txt deleted=true fav=0 size=10 hash= mime=",
      ),
    ).toContain("in the trash");
  });

  it("names a folder", () => {
    expect(describeRestoreDifference("only here: folder /Photos deleted=false fav=0")).toBe(
      "Only in this silo: the folder /Photos.",
    );
  });

  it("shortens a credential to its id prefix", () => {
    expect(
      describeRestoreDifference(
        "only here: password aaaaaaaa-0000-0000-0000-000000000001",
      ),
    ).toBe("Only in this silo: a credential entry (id aaaaaaaa…).");
  });

  it("explains a history length mismatch", () => {
    expect(describeRestoreDifference("only in the restore: ops count=12")).toContain(
      "different lengths",
    );
  });

  it("returns an unrecognised line untouched", () => {
    const raw = "only here: something this build does not know";
    expect(describeRestoreDifference(raw)).toBe(raw);
    expect(describeRestoreDifference("no prefix at all")).toBe("no prefix at all");
  });
});
