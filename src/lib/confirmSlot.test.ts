import { describe, expect, it } from "vitest";
import { decideConfirmSlot } from "./confirmSlot";

describe("the confirmation slot", () => {
  it("shows a question when nothing is on screen", () => {
    expect(decideConfirmSlot(false)).toEqual({ action: "show" });
  });

  it("keeps the question already on screen", () => {
    // The dangerous swap: a second request arriving while somebody is
    // reading the first replaced it in place, same position, same buttons.
    // They pressed Confirm on a question they never saw, and the one they
    // thought they had answered never ran.
    expect(decideConfirmSlot(true)).toEqual({ action: "decline" });
  });
});
