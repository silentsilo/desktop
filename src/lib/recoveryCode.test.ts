import { describe, expect, it } from "vitest";
import {
  CODE_LENGTH,
  distribute,
  fromGroups,
  isComplete,
  normalizeCode,
  replaceGroup,
  toGroups,
} from "./recoveryCode";

const WHOLE = "0123-4567-89AB-CDEF-GHJK-MNPQ-RSTV-WXYZ";

describe("normalizeCode", () => {
  it("folds the four characters the alphabet leaves out", () => {
    // Not cosmetic: someone reading their own handwriting off paper types
    // the letter they see, and refusing it would reject a correct code.
    expect(normalizeCode("OoIiLlUu")).toBe("001111VV");
  });

  it("drops whatever a code was written with", () => {
    expect(normalizeCode(" 0123-4567 \n 89ab ")).toBe("0123456789AB");
  });

  it("leaves a code that is already clean alone", () => {
    expect(normalizeCode(WHOLE)).toBe(WHOLE.replaceAll("-", ""));
  });
});

describe("toGroups and fromGroups", () => {
  it("round-trips a whole code", () => {
    expect(fromGroups(toGroups(WHOLE))).toBe(WHOLE);
  });

  it("always returns eight boxes, however little has been typed", () => {
    const groups = toGroups("01");
    expect(groups).toHaveLength(8);
    expect(groups[0]).toBe("01");
    expect(groups[7]).toBe("");
  });

  it("drops anything past the end of a code", () => {
    expect(normalizeCode(fromGroups(toGroups(WHOLE + "EXTRA")))).toHaveLength(CODE_LENGTH);
  });

  it("leaves no trailing dashes on a half-typed code", () => {
    expect(fromGroups(toGroups("0123"))).toBe("0123");
  });
});

describe("isComplete", () => {
  it("is true only for a whole code", () => {
    expect(isComplete(WHOLE)).toBe(true);
    expect(isComplete(WHOLE.slice(0, -1))).toBe(false);
    expect(isComplete("")).toBe(false);
  });

  it("does not count the dashes", () => {
    expect(isComplete(WHOLE.replaceAll("-", ""))).toBe(true);
  });
});

describe("distribute", () => {
  it("spreads a whole code pasted into the first box", () => {
    const { groups, focus } = distribute(toGroups(""), 0, WHOLE);
    expect(fromGroups(groups)).toBe(WHOLE);
    expect(focus).toBe(7);
  });

  it("spreads a code pasted into a later box, from there on", () => {
    // Someone clicks the box they are on and pastes the rest.
    const { groups } = distribute(toGroups(""), 6, "RSTVWXYZ");
    expect(groups[6]).toBe("RSTV");
    expect(groups[7]).toBe("WXYZ");
  });

  it("stops at the last box rather than losing the overflow silently", () => {
    const { groups, focus } = distribute(toGroups(""), 7, "WXYZ0000");
    expect(groups[7]).toBe("WXYZ");
    expect(focus).toBe(7);
  });

  it("carries on into the next box when this one fills up", () => {
    const { groups, focus } = distribute(toGroups("012"), 0, "34");
    expect(groups[0]).toBe("0123");
    expect(groups[1]).toBe("4");
    expect(focus).toBe(1);
  });

  it("moves the caret on as soon as a box is full", () => {
    const { focus } = distribute(toGroups("012"), 0, "3");
    expect(focus).toBe(1);
  });

  it("overwrites what was typed before rather than merging with it", () => {
    // The case that got through in the browser: a whole code pasted over a
    // half-typed one used to leave the old characters wedged between the
    // new ones, and the result looked plausible enough to submit.
    const halfTyped = toGroups("0123-01V");
    const { groups } = replaceGroup(halfTyped, 0, WHOLE);
    expect(fromGroups(groups)).toBe(WHOLE);
  });

  it("takes a pasted code with its dashes", () => {
    const { groups } = distribute(toGroups(""), 0, WHOLE);
    expect(groups[3]).toBe("CDEF");
  });
});

describe("replaceGroup", () => {
  it("overwrites the box rather than appending to it", () => {
    const { groups } = replaceGroup(toGroups(WHOLE), 0, "ZZZZ");
    expect(groups[0]).toBe("ZZZZ");
    expect(groups[1]).toBe("4567");
  });

  it("still spills forward when more than a group is typed at once", () => {
    const { groups, focus } = replaceGroup(toGroups(""), 0, "01234567");
    expect(groups[0]).toBe("0123");
    expect(groups[1]).toBe("4567");
    expect(focus).toBe(2);
  });
});
