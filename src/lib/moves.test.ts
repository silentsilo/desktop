import { describe, expect, it } from "vitest";
import { canMoveTo, moveSummary } from "./moves";
import type { VaultEntry } from "./types";

const folder = (id: string, path: string) =>
  ({ kind: "folder", id, path, name: path.split("/").pop() }) as unknown as VaultEntry;
const file = (id: string) => ({ kind: "file", id, name: `${id}.pdf` }) as unknown as VaultEntry;

describe("canMoveTo", () => {
  const docs = folder("d", "/Docs");

  it("lets files and folders go into another folder", () => {
    expect(canMoveTo([file("a"), docs], { id: "x", path: "/Archive" }, "/")).toBe(true);
  });

  it("refuses a folder onto itself or anywhere inside it", () => {
    expect(canMoveTo([docs], { id: "d", path: "/Docs" }, "/")).toBe(false);
    expect(canMoveTo([docs], { id: "i", path: "/Docs/2025" }, "/")).toBe(false);
    expect(canMoveTo([docs], { path: "/Docs/2025/x" }, "/")).toBe(false);
  });

  it("does not confuse a sibling whose name starts the same", () => {
    expect(canMoveTo([docs], { id: "o", path: "/Docs old" }, "/")).toBe(true);
  });

  it("refuses the folder they are already in, and an empty selection", () => {
    expect(canMoveTo([file("a")], { path: "/" }, "/")).toBe(false);
    expect(canMoveTo([], { path: "/Archive" }, "/")).toBe(false);
  });
});

describe("moveSummary", () => {
  it("counts what moved", () => {
    expect(moveSummary({ moved: 3, skipped: [], failed: [] }, "Archive")).toEqual({
      text: "Moved 3 items to Archive.",
      error: false,
    });
  });

  it("names what was skipped and what failed, and marks a failure as an error", () => {
    const s = moveSummary(
      {
        moved: 1,
        skipped: ["report.pdf"],
        failed: [{ name: "Docs", reason: "a folder cannot move into itself." }],
      },
      "Docs",
    );
    expect(s.error).toBe(true);
    expect(s.text).toContain("Moved 1 item to Docs.");
    expect(s.text).toContain('Left "report.pdf" where it was');
    expect(s.text).toContain('"Docs" did not move: a folder cannot move into itself.');
  });

  it("says so when nothing had to move", () => {
    expect(moveSummary({ moved: 0, skipped: [], failed: [] }, "Docs").text).toBe(
      "Nothing moved: everything is there already.",
    );
  });
});
