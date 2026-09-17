/// The question asked before an export replaces files already on disk.
///
/// Saving one file goes through the system's save dialog, and that dialog
/// asks about an existing name itself. Saving several files, or a folder,
/// does not go near it: the app wrote each destination path straight out,
/// so a folder of holiday photos saved twice silently replaced the first
/// set. This is the wording for the one question asked instead, before
/// anything is written.

/// Names the clash, listing enough of it to decide with.
///
/// Three names and a count, rather than all of them: a folder export can
/// collide on hundreds, and a dialog that scrolls is one nobody reads.
/// `total` is how many files the whole save covers, and is left out for a
/// folder export, where the subtree's size says nothing useful next to the
/// list of collisions.
export function overwriteMessage(clashes: string[], total?: number): string {
  if (clashes.length === 0) return "";
  const shown = clashes.slice(0, 3).join(", ");
  const rest = clashes.length - 3;
  const more = rest > 0 ? `, and ${rest} more` : "";
  const which =
    clashes.length === 1
      ? `“${clashes[0]}” is already in the folder you picked.`
      : total !== undefined && clashes.length < total
        ? `${clashes.length} of the ${total} files you are saving are already in the folder you picked: ${shown}${more}.`
        : `${clashes.length} files are already in the folder you picked: ${shown}${more}.`;
  return `${which} They are left alone unless you say otherwise.`;
}

/// The confirm button. It is the safe answer, so it says what happens when
/// the box below it is left unticked, and a save where everything collides
/// has nothing left to write.
export function overwriteConfirmLabel(clashCount: number, total?: number): string {
  if (total !== undefined && clashCount >= total) return "Save nothing";
  return "Save the rest";
}
