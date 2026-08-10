import { describe, expect, it } from "vitest";
import { type SiloReport, reportTime, siloReportText } from "./siloReport";

function report(overrides: Partial<SiloReport> = {}): SiloReport {
  return {
    app_version: "1.0.0",
    name: "Personal",
    path: "C:\\Users\\alex\\Documents\\SilentSilo\\Personal",
    silo_id: "8f14e45f-ceea-467a-9575-8bd3f1a2c4d5",
    marker_version: 1,
    formats: {
      silo_files: 1,
      marker: 1,
      index_schema: 1,
      sealed_payload: 1,
      blob: 1,
      recovery_envelope: 1,
    },
    files: [
      { name: "vault.db.enc", present: true, bytes: 1536, modified: 1755700000 },
      { name: "vault.db.enc.next", present: false, bytes: null, modified: null },
    ],
    keys: { total: 3, platform: 1, portable: 2, revoked: 1 },
    recovery_envelope: true,
    working_copy: null,
    sync_provider: null,
    disk_free_bytes: 1024 * 1024 * 1024,
    disk_total_bytes: 4 * 1024 * 1024 * 1024,
    ...overrides,
  };
}

describe("siloReportText", () => {
  it("says a file is missing rather than printing an empty size", () => {
    // The line a support thread is actually reading: a silo that will not
    // open is usually missing exactly one of these.
    const text = siloReportText(report());
    expect(text).toContain("vault.db.enc.next        missing");
  });

  it("never renders a null through to the reader", () => {
    // Every optional field at once. "null" or "undefined" in a report a user
    // pastes into an email is how a real answer gets mistaken for a bug.
    const text = siloReportText(
      report({
        silo_id: null,
        marker_version: null,
        keys: null,
        disk_free_bytes: null,
        disk_total_bytes: null,
        files: [{ name: "vault.salt", present: false, bytes: null, modified: null }],
      })
    );
    expect(text).not.toMatch(/null|undefined|NaN/);
    expect(text).toContain("no marker on disk");
    expect(text).toContain("cannot be read");
  });

  it("names a working copy left behind, because it explains the last crash", () => {
    const text = siloReportText(
      report({ working_copy: { name: "vault.db", present: true, bytes: 2048, modified: 1755700000 } })
    );
    expect(text).toContain("left behind");
    expect(text).toContain("2 KB");
  });

  it("reports a clean shutdown as such", () => {
    expect(siloReportText(report())).toContain("the last session closed cleanly");
  });

  it("counts keys without naming any of them", () => {
    const text = siloReportText(report());
    expect(text).toContain("3 enrolled (1 sealed to a machine, 2 portable, 1 retired)");
  });

  it("leaves retired keys out when there are none", () => {
    const text = siloReportText(report({ keys: { total: 2, platform: 0, portable: 2, revoked: 0 } }));
    expect(text).toContain("2 enrolled (0 sealed to a machine, 2 portable)");
    expect(text).not.toContain("retired");
  });
});

describe("reportTime", () => {
  it("stamps UTC explicitly, since the two ends of a support thread differ", () => {
    expect(reportTime(1755700000)).toBe("2025-08-20 14:26:40Z");
  });

  it("says unknown rather than inventing an epoch date", () => {
    // A filesystem that will not report a time would otherwise read as
    // 1970, which looks like corruption.
    expect(reportTime(null)).toBe("unknown");
  });
});
