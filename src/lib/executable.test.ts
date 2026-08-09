import { describe, expect, it } from "vitest";
import { runsOnOpen } from "./executable";

describe("runsOnOpen", () => {
  it("recognises what a double click executes", () => {
    expect(runsOnOpen("setup.exe")).toBe(true);
    expect(runsOnOpen("payload.hta")).toBe(true);
    expect(runsOnOpen("script.vbs")).toBe(true);
    expect(runsOnOpen("Shortcut.lnk")).toBe(true);
    expect(runsOnOpen("saver.SCR")).toBe(true);
  });

  it("recognises the ones that do not look like programs", () => {
    // Each of these opens something other than what its name suggests, and
    // each has been used in the wild for exactly that reason. A silo syncs,
    // so one of them can arrive from another device.
    expect(runsOnOpen("Invoice.appref-ms")).toBe(true);
    expect(runsOnOpen("update.application")).toBe(true);
    expect(runsOnOpen("photos.library-ms")).toBe(true);
    expect(runsOnOpen("fix.settingcontent-ms")).toBe(true);
    expect(runsOnOpen("helpdesk.diagcab")).toBe(true);
    expect(runsOnOpen("manual.chm")).toBe(true);
    expect(runsOnOpen("tool.jar")).toBe(true);
    expect(runsOnOpen("legacy.msh")).toBe(true);
  });

  it("leaves ordinary documents alone", () => {
    expect(runsOnOpen("contract.pdf")).toBe(false);
    expect(runsOnOpen("photo.jpg")).toBe(false);
    expect(runsOnOpen("notes.txt")).toBe(false);
    expect(runsOnOpen("archive.zip")).toBe(false);
  });

  it("sees through the trailing dots and spaces Windows strips", () => {
    // "payload.exe " opens the same program, so a name that looks harmless
    // in a list must not read as harmless here.
    expect(runsOnOpen("payload.exe ")).toBe(true);
    expect(runsOnOpen("payload.exe.")).toBe(true);
    expect(runsOnOpen("payload.exe . ")).toBe(true);
  });

  it("only looks at the last extension", () => {
    expect(runsOnOpen("report.exe.pdf")).toBe(false);
    expect(runsOnOpen("report.pdf.exe")).toBe(true);
  });

  it("treats a name with no extension as ordinary", () => {
    expect(runsOnOpen("README")).toBe(false);
    expect(runsOnOpen(".gitignore")).toBe(false);
    expect(runsOnOpen("")).toBe(false);
  });
});
