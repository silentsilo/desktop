import { afterEach, describe, expect, it } from "vitest";
import { explorerKeysBlocked } from "./explorerKeys";

type Fake = { tagName: string; isContentEditable?: boolean; inMenu?: boolean };

function key(key: string, target: Fake): KeyboardEvent {
  const el = {
    tagName: target.tagName,
    isContentEditable: target.isContentEditable ?? false,
    closest: (sel: string) => (sel === '[role="menu"]' && target.inMenu ? {} : null),
  };
  return { key, target: el } as unknown as KeyboardEvent;
}

function dialogOpen(open: boolean) {
  (globalThis as { document?: unknown }).document = {
    querySelector: (sel: string) => (open && sel === '[aria-modal="true"]' ? {} : null),
  };
}

afterEach(() => {
  delete (globalThis as { document?: unknown }).document;
});

describe("explorerKeysBlocked", () => {
  it("lets shortcuts through on the file list", () => {
    dialogOpen(false);
    expect(explorerKeysBlocked(key("Enter", { tagName: "DIV" }))).toBe(false);
    expect(explorerKeysBlocked(key("Delete", { tagName: "BODY" }))).toBe(false);
  });

  it("leaves typing alone", () => {
    dialogOpen(false);
    expect(explorerKeysBlocked(key("Backspace", { tagName: "INPUT" }))).toBe(true);
    expect(explorerKeysBlocked(key("Delete", { tagName: "DIV", isContentEditable: true }))).toBe(true);
  });

  it("stops every shortcut while a dialog is open", () => {
    dialogOpen(true);
    expect(explorerKeysBlocked(key("Enter", { tagName: "DIV" }))).toBe(true);
    expect(explorerKeysBlocked(key("Delete", { tagName: "BODY" }))).toBe(true);
  });

  it("keeps Enter for a focused button or a menu", () => {
    dialogOpen(false);
    expect(explorerKeysBlocked(key("Enter", { tagName: "BUTTON" }))).toBe(true);
    expect(explorerKeysBlocked(key("ArrowDown", { tagName: "DIV", inMenu: true }))).toBe(true);
    expect(explorerKeysBlocked(key("Delete", { tagName: "BUTTON" }))).toBe(false);
  });
});
