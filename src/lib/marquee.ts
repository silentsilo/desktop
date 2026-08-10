export type MarqueeBox = { x0: number; y0: number; x1: number; y1: number };
export type Rect = { left: number; right: number; top: number; bottom: number };

/** Normalizes a drag gesture's start point and current pointer position into
 * a box with min/max corners, regardless of which direction the drag went. */
export function computeMarqueeBox(startX: number, startY: number, curX: number, curY: number): MarqueeBox {
  return {
    x0: Math.min(startX, curX),
    y0: Math.min(startY, curY),
    x1: Math.max(startX, curX),
    y1: Math.max(startY, curY),
  };
}

/** Standard axis-aligned bounding box overlap test (strict, not touching). */
export function rectIntersectsBox(r: Rect, box: MarqueeBox): boolean {
  return r.left < box.x1 && r.right > box.x0 && r.top < box.y1 && r.bottom > box.y0;
}
