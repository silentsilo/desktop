/**
 * Walks the rendered page and reports anything a user could not read.
 *
 * Written because a contrast failure is invisible to whoever wrote the CSS —
 * you see the colour you intended, not the colour that won the cascade — and
 * because hover and disabled states are only reachable by actually being in
 * them. A rule that sets a dark background without setting a colour to match
 * looks fine until the pointer lands on it.
 *
 * Development only; the release build drops this with the rest of `dev/`.
 */

type Rgb = [number, number, number];
type Rgba = { rgb: Rgb; alpha: number };

function parseColor(value: string): Rgba | null {
  const m = value.match(/rgba?\(([^)]+)\)/);
  if (!m) return null;
  const parts = m[1]!.split(/[,/\s]+/).filter(Boolean).map(Number);
  if (parts.length < 3 || parts.some(Number.isNaN)) return null;
  return { rgb: [parts[0]!, parts[1]!, parts[2]!], alpha: parts.length > 3 ? parts[3]! : 1 };
}

/**
 * A gradient, reduced to the average of its colour stops.
 *
 * Rough on purpose: the point is to know roughly how light the surface is,
 * and a button's gradient never spans light and dark. Ignoring it entirely
 * would be much worse — it would report the card behind the button as the
 * background and pass a white-on-violet label as white-on-near-black.
 */
function averageGradientColor(image: string): Rgba | null {
  const stops = [...image.matchAll(/rgba?\([^)]+\)/g)]
    .map((m) => parseColor(m[0]))
    .filter((c): c is Rgba => c !== null && c.alpha > 0);
  if (stops.length === 0) return null;
  const avg = (i: number) => Math.round(stops.reduce((t, c) => t + c.rgb[i]!, 0) / stops.length);
  // The average alpha matters as much as the average hue. The page's
  // atmosphere is a violet wash at 22% over near-black; calling that an
  // opaque violet reported every heading on the app as failing against a
  // background nobody actually sees.
  const alpha = stops.reduce((t, c) => t + c.alpha, 0) / stops.length;
  return { rgb: [avg(0), avg(1), avg(2)], alpha };
}

/**
 * Known limitation: `color-mix()` computes to `oklab(...)`, which this does
 * not parse. Such a surface is skipped and the walk continues to the layer
 * behind it — which understates translucent tints slightly, and is the safe
 * direction to be wrong in. Reading a colour the tool cannot express would
 * be worse than reading the one behind it.
 */
function surfaceOf(el: Element): Rgba | null {
  const style = getComputedStyle(el);
  // A background image paints over the background colour, so when both are
  // set the gradient is what the eye lands on. Reading the colour first made
  // a white-on-violet button — violet gradient, 3% white beneath it — report
  // as white on white at 1:1, which is a failure that isn't there.
  if (style.backgroundImage !== "none") {
    const gradient = averageGradientColor(style.backgroundImage);
    if (gradient) return gradient;
  }
  const color = parseColor(style.backgroundColor);
  if (color && color.alpha > 0) return color;
  return null;
}

function over(top: Rgba, bottom: Rgb): Rgb {
  return [0, 1, 2].map((i) =>
    Math.round(top.rgb[i]! * top.alpha + bottom[i]! * (1 - top.alpha)),
  ) as Rgb;
}

/**
 * The colour actually painted behind an element.
 *
 * Composites the translucent layers rather than stopping at the first one:
 * the auth card is white at 3% over a near-black page, and treating that as
 * white made every reading on it meaningless — the first version of this
 * audit reported the whole screen as failing, which is its own kind of
 * uselessness.
 */
function effectiveBackground(el: Element): Rgb {
  const layers: Rgba[] = [];
  let node: Element | null = el.parentElement;
  while (node) {
    const surface = surfaceOf(node);
    if (surface) {
      layers.push(surface);
      if (surface.alpha >= 1) break;
    }
    node = node.parentElement;
  }
  // Painted back to front, starting from the deepest opaque layer.
  const deepest = layers[layers.length - 1];
  let result: Rgb = deepest && deepest.alpha >= 1 ? layers.pop()!.rgb : [0, 0, 0];
  for (const layer of layers.reverse()) result = over(layer, result);

  // The element's own background sits on top of everything behind it.
  const own = surfaceOf(el);
  return own ? over(own, result) : result;
}

function relativeLuminance([r, g, b]: Rgb): number {
  const f = (c: number) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

export function contrastRatio(a: Rgb, b: Rgb): number {
  const [hi, lo] = [relativeLuminance(a), relativeLuminance(b)].sort((x, y) => y - x);
  return (hi! + 0.05) / (lo! + 0.05);
}

export type Finding = {
  selector: string;
  text: string;
  state: string;
  ratio: number;
  fg: string;
  bg: string;
};

function describe(el: Element): string {
  const cls = typeof el.className === "string" ? el.className.trim().split(/\s+/).join(".") : "";
  return el.tagName.toLowerCase() + (cls ? `.${cls}` : "");
}

/**
 * WCAG AA is 4.5 for body text and 3.0 for large text. Interactive controls
 * are held to the body threshold regardless of size: a label you squint at
 * is a label you misread, and these are the things people click.
 */
const MIN_RATIO = 4.5;

function check(el: HTMLElement, state: string, findings: Finding[]) {
  const style = getComputedStyle(el);
  const fg = parseColor(style.color);
  if (!fg) return;
  const bg = effectiveBackground(el);
  // Text with its own transparency is composited over what it sits on
  // before being compared with it.
  const ratio = contrastRatio(over(fg, bg), bg);
  if (ratio >= MIN_RATIO) return;
  findings.push({
    selector: describe(el),
    text: (el.innerText || el.getAttribute("aria-label") || "").trim().slice(0, 40),
    state,
    ratio: Math.round(ratio * 100) / 100,
    fg: style.color,
    bg: `rgb(${bg.join(", ")})`,
  });
}

/**
 * Forces each state by injecting a stylesheet that re-applies the `:hover`
 * and `:focus-visible` rules unconditionally. Dispatching pointer events
 * cannot do this — the CSS `:hover` pseudo-class only responds to a real
 * cursor, so scripted hovers report the resting colours and quietly pass.
 */
function withForcedState(state: "hover" | "focus", run: () => void) {
  const sheet = document.createElement("style");
  const pseudo = state === "hover" ? ":hover" : ":focus-visible";
  const rules = [...document.styleSheets]
    .flatMap((s) => {
      try {
        return [...(s.cssRules as unknown as CSSRule[])];
      } catch {
        return [];
      }
    })
    .filter((r): r is CSSStyleRule => r instanceof CSSStyleRule && r.selectorText.includes(pseudo))
    .map((r) => `${r.selectorText.replaceAll(pseudo, "")} { ${r.style.cssText} }`);

  sheet.textContent = rules.join("\n");
  document.head.append(sheet);
  try {
    run();
  } finally {
    sheet.remove();
  }
}

const INTERACTIVE = "button, a, input, select, textarea, [role='button'], .silo-item, .tab-item";

export function auditContrast(): Finding[] {
  const findings: Finding[] = [];
  const els = [...document.querySelectorAll<HTMLElement>(INTERACTIVE)].filter(
    (el) => el.offsetParent !== null || getComputedStyle(el).position === "fixed",
  );

  for (const el of els) check(el, "rest", findings);
  withForcedState("hover", () => {
    for (const el of els) check(el, "hover", findings);
  });
  withForcedState("focus", () => {
    for (const el of els) check(el, "focus", findings);
  });

  // Body text too — the same cascade accidents hit paragraphs.
  for (const el of document.querySelectorAll<HTMLElement>("p, h1, h2, h3, li, strong, span.hint")) {
    if (el.offsetParent === null || !el.innerText.trim()) continue;
    check(el, "rest", findings);
  }
  return findings;
}

declare global {
  interface Window {
    __auditContrast?: () => Finding[];
  }
}

export function installContrastAudit() {
  window.__auditContrast = auditContrast;
}
