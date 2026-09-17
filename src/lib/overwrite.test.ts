import { describe, expect, it } from "vitest";
import { overwriteConfirmLabel, overwriteMessage } from "./overwrite";

describe("the overwrite question", () => {
  it("names the one file when there is one", () => {
    expect(overwriteMessage(["notes.txt"], 4)).toBe(
      "“notes.txt” is already in the folder you picked. They are left alone unless you say otherwise.",
    );
  });

  it("says how many of the save the clashes are", () => {
    expect(overwriteMessage(["a.txt", "b.txt"], 5)).toBe(
      "2 of the 5 files you are saving are already in the folder you picked: a.txt, b.txt. They are left alone unless you say otherwise.",
    );
  });

  it("lists three and counts the rest", () => {
    expect(overwriteMessage(["a", "b", "c", "d", "e"], 9)).toContain("a, b, c, and 2 more.");
  });

  it("drops the fraction when every file in the save clashes", () => {
    expect(overwriteMessage(["a.txt", "b.txt"], 2)).toContain(
      "2 files are already in the folder you picked",
    );
  });

  it("drops the fraction for a folder, where the subtree size says nothing", () => {
    expect(overwriteMessage(["Invoices\\a.pdf", "Invoices\\b.pdf"])).toContain(
      "2 files are already in the folder you picked",
    );
  });

  it("has nothing to say when nothing clashes", () => {
    expect(overwriteMessage([], 3)).toBe("");
  });
});

describe("the confirm button on that question", () => {
  it("offers the rest when there is a rest", () => {
    expect(overwriteConfirmLabel(2, 5)).toBe("Save the rest");
  });

  it("does not promise a save when every file would be skipped", () => {
    expect(overwriteConfirmLabel(3, 3)).toBe("Save nothing");
  });

  it("assumes there is a rest when the total is unknown", () => {
    expect(overwriteConfirmLabel(3)).toBe("Save the rest");
  });
});
