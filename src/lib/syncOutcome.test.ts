import { describe, expect, it } from "vitest";
import { describeSync, syncOutcome, type SyncReport, type TargetStatus } from "./syncOutcome";

function report(over: Partial<SyncReport> = {}): SyncReport {
  return {
    configured: true,
    ops_pushed: 0,
    ops_fetched: 0,
    ops_applied: 0,
    blobs_uploaded: 0,
    blobs_failed: 0,
    renamed: [],
    needs_rebuild: false,
    compacted: 0,
    targets: [],
    ...over,
  };
}

function target(over: Partial<TargetStatus> = {}): TargetStatus {
  return {
    id: "t1",
    label: "SFTP",
    ops_pushed: 0,
    blobs_uploaded: 0,
    failed: null,
    last_success: 0,
    retry_in: 0,
    waiting: false,
    ops_behind: 0,
    ...over,
  };
}

describe("a pass that did nothing", () => {
  it("says so only when there is nothing to say", () => {
    expect(describeSync(report({ targets: [target()] }))).toBe("Already up to date.");
  });

  it("does not claim to be up to date when it stood down for another pass", () => {
    // The reported bug: the button answered instantly while a background pass
    // was still uploading, and the files landed ten seconds later.
    expect(describeSync(report({ skipped: true }))).toBe("A backup was already running.");
  });

  it("keeps saying that even with a target on the list", () => {
    expect(describeSync(report({ skipped: true, targets: [target()] }))).toBe(
      "A backup was already running.",
    );
  });

  it("says nothing was sent when a target is behind", () => {
    expect(describeSync(report({ targets: [target({ ops_behind: 3 })] }))).toBe(
      "Nothing was sent.",
    );
  });
});

describe("a pass that moved something", () => {
  it("counts what went out", () => {
    expect(describeSync(report({ ops_pushed: 1, blobs_uploaded: 2 }))).toBe(
      "1 change sent, 2 files backed up",
    );
  });

  it("reports work rather than the stand-down, if both are somehow set", () => {
    expect(describeSync(report({ skipped: true, ops_pushed: 4 }))).toBe("4 changes sent");
  });

  it("mentions retries and tidying", () => {
    expect(describeSync(report({ blobs_failed: 1, compacted: 5 }))).toBe(
      "1 failed, will retry, history tidied (5 old records dropped)",
    );
  });

  it("puts a stale device ahead of every counter", () => {
    const r = report({ needs_rebuild: true, ops_pushed: 9 });
    expect(describeSync(r)).toContain("too far behind");
  });
});

/// The line the panel would show. Only "idle" carries no message, and no
/// pass produces it.
function line(r: SyncReport, renamed = ""): string {
  const status = syncOutcome(r, renamed);
  if (status.kind === "idle") throw new Error("a finished pass is never idle");
  return status.message;
}

describe("the pass as a status", () => {
  it("reads a failure as one", () => {
    const r = report({ targets: [target({ failed: "host unreachable" })] });
    expect(syncOutcome(r, "")).toEqual({ kind: "error", message: "host unreachable" });
  });

  it("names the targets when more than one is configured", () => {
    const r = report({
      targets: [target({ failed: "refused" }), target({ id: "t2", label: "S3" })],
    });
    expect(line(r)).toBe("SFTP: refused");
  });

  it("keeps the partial result in front of the failure", () => {
    const r = report({
      ops_pushed: 2,
      targets: [target({ failed: "timed out" }), target({ id: "t2", label: "S3" })],
    });
    expect(line(r)).toBe("2 changes sent SFTP: timed out");
  });

  it("says when a target is deliberately being left alone", () => {
    const r = report({ targets: [target({ waiting: true, retry_in: 300 })] });
    expect(syncOutcome(r, "")).toEqual({
      kind: "ok",
      message: "Waiting before trying again, about 5 min.",
    });
  });

  it("carries a stand-down through as an ordinary result", () => {
    expect(syncOutcome(report({ skipped: true }), "")).toEqual({
      kind: "ok",
      message: "A backup was already running.",
    });
  });

  it("appends the rename notice", () => {
    expect(line(report({ ops_pushed: 1 }), " (2 renamed)")).toBe(
      "1 change sent (2 renamed)",
    );
  });

  it("reads a pass that opened no target at all as a failure", () => {
    // The shape the pass now reports when it gives up before touching
    // anything: every counter is zero because nothing was attempted, not
    // because there was nothing to do. That report used to be withheld
    // entirely, and the screen went on showing whatever the last good pass
    // had left there.
    const r = report({
      targets: [
        target({ failed: "the folder is not reachable" }),
        target({ id: "t2", label: "S3", failed: "connection refused" }),
      ],
    });
    expect(syncOutcome(r, "").kind).toBe("error");
  });

  it("does not treat a device that fell behind a compaction as a good pass", () => {
    // Sync is stopped in both directions until the device is set up again,
    // so the one thing the indicator must not do is show a tick.
    expect(describeSync(report({ needs_rebuild: true }))).toBe(
      "This device is too far behind to catch up. It has to be set up again from the current state.",
    );
  });

  it("a device the key moved on from reads as an error, not a quiet pass", () => {
    const r = report({ needs_rejoin: true });
    expect(syncOutcome(r, "").kind).toBe("error");
    expect(describeSync(r)).toContain("rejoin");
  });
});
