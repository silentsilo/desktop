import { describe, expect, it } from "vitest";
import { computeMarqueeBox, rectIntersectsBox } from "./marquee";

describe("computeMarqueeBox", () => {
  it("normalizes a drag going down-right", () => {
    expect(computeMarqueeBox(10, 10, 50, 80)).toEqual({ x0: 10, y0: 10, x1: 50, y1: 80 });
  });

  it("normalizes a drag going up-left (reversed corners)", () => {
    expect(computeMarqueeBox(50, 80, 10, 10)).toEqual({ x0: 10, y0: 10, x1: 50, y1: 80 });
  });

  it("normalizes a drag with mixed direction on each axis", () => {
    expect(computeMarqueeBox(50, 10, 10, 80)).toEqual({ x0: 10, y0: 10, x1: 50, y1: 80 });
  });

  it("collapses to a zero-size box for a click with no movement", () => {
    expect(computeMarqueeBox(20, 20, 20, 20)).toEqual({ x0: 20, y0: 20, x1: 20, y1: 20 });
  });
});

describe("rectIntersectsBox", () => {
  const box = { x0: 0, y0: 0, x1: 100, y1: 100 };

  it("is true when the rect is fully inside the box", () => {
    expect(rectIntersectsBox({ left: 10, right: 20, top: 10, bottom: 20 }, box)).toBe(true);
  });

  it("is true when the rect only partially overlaps the box", () => {
    expect(rectIntersectsBox({ left: 90, right: 150, top: 90, bottom: 150 }, box)).toBe(true);
  });

  it("is true when the rect fully contains the box", () => {
    expect(rectIntersectsBox({ left: -50, right: 150, top: -50, bottom: 150 }, box)).toBe(true);
  });

  it("is false when the rect is entirely outside the box", () => {
    expect(rectIntersectsBox({ left: 200, right: 250, top: 200, bottom: 250 }, box)).toBe(false);
  });

  it("is false when the rect merely touches the box's edge (strict overlap only)", () => {
    expect(rectIntersectsBox({ left: 100, right: 150, top: 0, bottom: 50 }, box)).toBe(false);
  });

  it("is false for a zero-size box sitting exactly on a rect's edge", () => {
    // The strict inequalities mean a degenerate box only overlaps a rect that
    // strictly contains its point — sitting right on the boundary doesn't count.
    const zeroBox = { x0: 20, y0: 20, x1: 20, y1: 20 };
    expect(rectIntersectsBox({ left: 20, right: 30, top: 10, bottom: 30 }, zeroBox)).toBe(false);
  });

  it("is true for a zero-size box whose point is strictly inside a rect", () => {
    const zeroBox = { x0: 20, y0: 20, x1: 20, y1: 20 };
    expect(rectIntersectsBox({ left: 10, right: 30, top: 10, bottom: 30 }, zeroBox)).toBe(true);
  });
});
