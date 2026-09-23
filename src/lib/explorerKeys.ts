/// Whether an explorer shortcut should leave this key alone.
///
/// Typing into a field is one case. A dialog or menu being open is the other:
/// the handler sits on `window`, so Enter on a dialog's button used to open
/// (decrypt) the file selected behind it instead of pressing the button, and
/// Delete moved that file to the trash. A focused button or link keeps its
/// own Enter as well.
export function explorerKeysBlocked(e: KeyboardEvent): boolean {
  const target = e.target as HTMLElement | null;
  if (
    target &&
    (target.tagName === "INPUT" ||
      target.tagName === "TEXTAREA" ||
      target.tagName === "SELECT" ||
      target.isContentEditable ||
      (e.key === "Enter" && (target.tagName === "BUTTON" || target.tagName === "A")) ||
      target.closest?.('[role="menu"]'))
  ) {
    return true;
  }
  return document.querySelector('[aria-modal="true"]') !== null;
}
