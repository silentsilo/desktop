import { describe, expect, it } from "vitest";
import { ModalStack } from "./useModal";

describe("ModalStack", () => {
  it("gives the keys to the dialog opened last", () => {
    const stack = new ModalStack();
    const lower = {};
    const upper = {};
    stack.push(lower);
    stack.push(upper);
    expect(stack.isTop(upper)).toBe(true);
    expect(stack.isTop(lower)).toBe(false);
  });

  it("hands them back when the top one closes", () => {
    const stack = new ModalStack();
    const lower = {};
    const upper = {};
    stack.push(lower);
    stack.push(upper);
    stack.remove(upper);
    expect(stack.isTop(lower)).toBe(true);
  });

  it("keeps the top one on top when one underneath closes", () => {
    const stack = new ModalStack();
    const lower = {};
    const upper = {};
    stack.push(lower);
    stack.push(upper);
    stack.remove(lower);
    expect(stack.isTop(upper)).toBe(true);
    expect(stack.isTop(lower)).toBe(false);
  });
});
