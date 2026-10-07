import { describe, expect, it } from "vitest";
import { openingPercent, openingTitle, type Opening } from "./OpenProgress";

const at = (phase: Opening["phase"], done: number, total: number): Opening => ({
  fileId: "f",
  name: "Holiday.mkv",
  phase,
  done,
  total,
});

describe("opening a file", () => {
  it("names the step it is on", () => {
    expect(openingTitle(at("preparing", 0, 0))).toBe("Getting Holiday.mkv ready");
    expect(openingTitle(at("downloading", 1, 2))).toBe("Downloading Holiday.mkv");
    expect(openingTitle(at("decrypting", 1, 2))).toBe("Decrypting Holiday.mkv");
  });

  it("says how far it is only when it can tell, and never 100 before it ends", () => {
    expect(openingPercent(at("preparing", 0, 0))).toBeNull();
    expect(openingPercent(at("decrypting", 0, 0))).toBeNull();
    expect(openingPercent(at("decrypting", 600, 1000))).toBe(60);
    expect(openingPercent(at("decrypting", 1000, 1000))).toBe(99);
    expect(openingPercent(at("opening", 1000, 1000))).toBe(100);
  });
});
