import { describe, expect, it } from "vitest";
import {
  copyState,
  currentCopies,
  describeDuration,
  protectionWarning,
  type BackupTargetView,
} from "./copies";

const NOW = 1_800_000_000;

function target(over: Partial<BackupTargetView> = {}): BackupTargetView {
  return {
    id: "t1",
    label: "Bucket",
    config: { kind: "folder", path: "/backup" } as BackupTargetView["config"],
    primary: true,
    last_success: NOW,
    ops_behind: 0,
    retry_in: 0,
    archive: false,
    hosted: false,
    ...over,
  };
}

describe("describeDuration", () => {
  it("says it the way a person would", () => {
    expect(describeDuration(0)).toBe("a moment");
    expect(describeDuration(45)).toBe("45 seconds");
    expect(describeDuration(60)).toBe("a minute");
    expect(describeDuration(3600)).toBe("an hour");
    expect(describeDuration(47 * 24 * 3600)).toBe("2 months");
  });

  it("never reports a negative age", () => {
    // A target written a second into the future is a clock that disagrees
    // with itself, not a copy that will exist tomorrow.
    expect(describeDuration(-500)).toBe("a moment");
  });
});

describe("copyState", () => {
  it("calls a copy that has everything up to date", () => {
    const state = copyState(target({ last_success: NOW - 120 }), NOW);
    expect(state.health).toBe("current");
    expect(state.detail).toBe("Last written 2 minutes ago.");
  });

  it("separates never written from long unwritten", () => {
    expect(copyState(target({ last_success: 0 }), NOW).health).toBe("never");
    expect(copyState(target({ last_success: NOW - 47 * 24 * 3600 }), NOW).health).toBe("stale");
  });

  it("leads with the age once a copy has gone stale, backlog or not", () => {
    // A disk unplugged since spring is not "3 changes behind", it is a copy
    // that stopped being one. Saying the small number first would let
    // someone read it as nearly fine.
    const state = copyState(
      target({ last_success: NOW - 60 * 24 * 3600, ops_behind: 3 }),
      NOW,
    );
    expect(state.health).toBe("stale");
    expect(state.headline).toBe("Last written 2 months ago");
    expect(state.detail).toBe("3 changes not there yet.");
  });

  it("mentions the wait only while there is one", () => {
    const waiting = copyState(target({ ops_behind: 2, retry_in: 600 }), NOW);
    expect(waiting.detail).toContain("Next attempt in 10 minutes.");
    const due = copyState(target({ ops_behind: 2, retry_in: 0 }), NOW);
    expect(due.detail).not.toContain("Next attempt");
  });
});

describe("currentCopies", () => {
  it("counts only the ones that actually hold everything now", () => {
    const targets = [
      target({ id: "a" }),
      target({ id: "b", last_success: NOW - 90 * 24 * 3600 }),
      target({ id: "c", ops_behind: 4 }),
    ];
    expect(currentCopies(targets, NOW)).toBe(1);
  });
});

describe("protectionWarning", () => {
  it("says nothing when the bucket really does refuse deletes", () => {
    expect(protectionWarning({ versioning: true, object_lock: true }, true)).toBe("");
  });

  it("calls out versioning without object lock, because it looks like protection", () => {
    // A delete on a versioned bucket succeeds and leaves the old version
    // readable, so revoking a key appears to have worked when it has not.
    const warning = protectionWarning({ versioning: true, object_lock: false }, true);
    expect(warning).toContain("revoking a key");
  });

  it("warns when the place reports nothing at all", () => {
    expect(protectionWarning({ versioning: false, object_lock: false }, true)).toContain(
      "no object lock",
    );
  });

  it("stays quiet for a working target, whatever the bucket says", () => {
    expect(protectionWarning({ versioning: false, object_lock: false }, false)).toBe("");
  });
});
