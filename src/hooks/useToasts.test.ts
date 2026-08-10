import { describe, expect, it } from "vitest";
import { MAX_VISIBLE, plan, toastDuration } from "./useToasts";
import type { Toast } from "../lib/types";
import { isLockedError } from "../lib/errors";

const toast = (id: string, message: string, kind: Toast["kind"] = "error"): Toast => ({
  id,
  kind,
  message,
});

describe("plan", () => {
  it("says the same thing once, however many times it happens", () => {
    // Locking mid-operation fails every file the same way, which used to
    // put one identical toast per file on screen.
    const shown = [toast("t-1", "vault is locked")];

    const decided = plan(shown, toast("t-2", "vault is locked"));

    expect(decided).toEqual({ action: "repeat", id: "t-1" });
  });

  it("treats the same words of a different kind as its own message", () => {
    const shown = [toast("t-1", "Saved.", "success")];

    const decided = plan(shown, toast("t-2", "Saved.", "error"));

    expect(decided.action).toBe("add");
  });

  it("keeps different messages side by side", () => {
    const shown = [toast("t-1", "vault is locked")];

    const decided = plan(shown, toast("t-2", "Added 1 item.", "success"));

    expect(decided).toEqual({
      action: "add",
      list: [shown[0]!, toast("t-2", "Added 1 item.", "success")],
      evicted: [],
    });
  });

  it("drops the oldest rather than growing without end", () => {
    const shown = Array.from({ length: MAX_VISIBLE }, (_, i) => toast(`t-${i}`, `message ${i}`));

    const decided = plan(shown, toast("t-new", "newest"));

    expect(decided.action).toBe("add");
    if (decided.action !== "add") return;
    expect(decided.list).toHaveLength(MAX_VISIBLE);
    expect(decided.list[MAX_VISIBLE - 1]!.message).toBe("newest");
    // Named, so the caller can clear the timer of what it just removed.
    expect(decided.evicted).toEqual([shown[0]!]);
  });

  it("lets a message return once it is off screen", () => {
    const decided = plan([], toast("t-9", "vault is locked"));

    expect(decided.action).toBe("add");
  });
});

describe("toastDuration", () => {
  it("gives a long message longer, within bounds", () => {
    expect(toastDuration("Saved.")).toBe(5_500);
    expect(toastDuration("word ".repeat(60))).toBe(15_000);
  });
});

describe("the locked silo", () => {
  it("is not reported: locking is what the user just asked for", () => {
    // In-flight reads racing a lock used to put a row of identical errors
    // on screen, saying the thing the user had done a moment earlier.
    expect(isLockedError("vault is locked")).toBe(true);
    expect(isLockedError(new Error("invoke(vault_blob_status): vault is locked"))).toBe(true);
  });

  it("does not swallow anything else", () => {
    expect(isLockedError("bucket does not exist")).toBe(false);
    expect(isLockedError(null)).toBe(false);
  });
});
