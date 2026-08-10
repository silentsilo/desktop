import { describe, expect, it } from "vitest";
import { PasswordRefreshGate } from "./passwordRefresh";

describe("re-reading the credential store", () => {
  it("reads straight away when nothing is being written", () => {
    // The ordinary case, and the one that was missing entirely: a sync pass
    // applied a login changed on another device and the panel never heard
    // about it, for as long as the app stayed open.
    expect(new PasswordRefreshGate().requestRefresh()).toBe(true);
  });

  it("waits while a local write is in flight", () => {
    // The panel updates its list before the vault write returns. Reading
    // between the two shows the row as it was, which looks exactly like the
    // edit having been thrown away.
    const gate = new PasswordRefreshGate();
    gate.beginWrite();
    expect(gate.requestRefresh()).toBe(false);
  });

  it("settles the deferred read once the last write finishes", () => {
    const gate = new PasswordRefreshGate();
    gate.beginWrite();
    gate.requestRefresh();
    expect(gate.endWrite()).toBe(true);
  });

  it("waits for every write, not just the first", () => {
    // Importing writes one entry per call, so several are in flight at once
    // and only the last of them may let the reload through.
    const gate = new PasswordRefreshGate();
    gate.beginWrite();
    gate.beginWrite();
    gate.requestRefresh();
    expect(gate.endWrite()).toBe(false);
    expect(gate.endWrite()).toBe(true);
  });

  it("does not invent a read nobody asked for", () => {
    const gate = new PasswordRefreshGate();
    gate.beginWrite();
    expect(gate.endWrite()).toBe(false);
  });

  it("owes only one read however many arrive during a write", () => {
    const gate = new PasswordRefreshGate();
    gate.beginWrite();
    gate.requestRefresh();
    gate.requestRefresh();
    expect(gate.endWrite()).toBe(true);
    expect(gate.endWrite()).toBe(false);
  });
});
