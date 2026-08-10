/** The recovery code as it is written down: eight groups of four, the same
 *  shape the printed kit uses. */

export const GROUP_COUNT = 8;
export const GROUP_SIZE = 4;
export const CODE_LENGTH = GROUP_COUNT * GROUP_SIZE;

/**
 * What the user typed, reduced to what could have been generated. Crockford's
 * alphabet leaves out I, L, O and U, so those fold to 1, 1, 0 and V rather
 * than being refused. Everything else non-alphanumeric is dropped.
 */
export function normalizeCode(input: string): string {
  const out: string[] = [];
  for (const raw of input) {
    if (!/[0-9a-zA-Z]/.test(raw)) continue;
    const c = raw.toUpperCase();
    if (c === "O") out.push("0");
    else if (c === "I" || c === "L") out.push("1");
    else if (c === "U") out.push("V");
    else out.push(c);
  }
  return out.join("");
}

/** The code split into the boxes, padded out so there are always eight. */
export function toGroups(code: string): string[] {
  const chars = normalizeCode(code).slice(0, CODE_LENGTH);
  return Array.from({ length: GROUP_COUNT }, (_, i) =>
    chars.slice(i * GROUP_SIZE, (i + 1) * GROUP_SIZE)
  );
}

/** The boxes back into one code, dashed like the printed one. */
export function fromGroups(groups: string[]): string {
  return groups.join("-").replace(/-+$/, "");
}

/** Whether every box is full, which is the only state worth submitting. */
export function isComplete(code: string): boolean {
  return normalizeCode(code).length === CODE_LENGTH;
}

/** Text arriving at one box, spilling into the ones after it. Returns where
 *  the caret goes, so typing never stops to wait for a click. */
export function distribute(
  groups: string[],
  at: number,
  text: string
): { groups: string[]; focus: number } {
  const incoming = normalizeCode(text);
  const next = [...groups];
  let cursor = at;

  for (let i = 0; i < incoming.length && cursor < GROUP_COUNT; ) {
    // Only the box the text arrived in keeps what it had. Merging into the
    // rest would turn a pasted code into one that is neither.
    const kept = cursor === at ? next[cursor]! : "";
    const room = GROUP_SIZE - kept.length;
    if (room === 0) {
      cursor += 1;
      continue;
    }
    next[cursor] = kept + incoming.slice(i, i + room);
    i += room;
    if (next[cursor]!.length === GROUP_SIZE) cursor += 1;
  }

  // A full last box keeps the caret rather than losing it off the end.
  return { groups: next, focus: Math.min(cursor, GROUP_COUNT - 1) };
}

/** A box replaced outright, with overflow still running forward. */
export function replaceGroup(
  groups: string[],
  at: number,
  text: string
): { groups: string[]; focus: number } {
  const cleared = [...groups];
  cleared[at] = "";
  return distribute(cleared, at, text);
}
