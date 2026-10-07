import { describe, expect, it } from "vitest";
import type { BackupTargetView } from "./copies";
import { copyLines, describeSyncState, typeLabel } from "./fileDetails";

function target(id: string, label: string): BackupTargetView {
  return {
    id,
    label,
    config: { kind: "folder", path: `/copies/${id}` },
    primary: false,
    last_success: 0,
    ops_behind: 0,
    retry_in: 0,
    archive: false,
  } as BackupTargetView;
}

describe("file details", () => {
  it("names the type by the extension", () => {
    expect(typeLabel("Photo 2026-09-14 1423.jpg")).toBe("JPG file");
    expect(typeLabel("README")).toBe("File");
    expect(typeLabel(".gitignore")).toBe("File");
  });

  it("says which copies hold the file, by the names the Copies screen uses", () => {
    const lines = copyLines(
      [target("a", "NAS"), target("b", ""), target("c", "OneDrive")],
      [
        { id: "a", held: true },
        { id: "b", held: false },
      ],
    );
    expect(lines).toEqual([
      { id: "a", name: "NAS", state: "holds" },
      { id: "b", name: "/copies/b", state: "owed" },
      { id: "c", name: "OneDrive", state: "unknown" },
    ]);
  });

  it("knows nothing of the copies for a file only in backup storage", () => {
    const lines = copyLines([target("a", "NAS")], [{ id: "a", held: null }]);
    expect(lines[0].state).toBe("unknown");
    expect(describeSyncState("remote-only")).toContain("backup storage only");
  });
});
