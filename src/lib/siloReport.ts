import { formatBytes } from "./format";

export type FileFact = {
  name: string;
  present: boolean;
  bytes: number | null;
  modified: number | null;
};

export type KeyCounts = {
  total: number;
  platform: number;
  portable: number;
  revoked: number;
};

export type Formats = {
  silo_files: number;
  marker: number;
  index_schema: number;
  sealed_payload: number;
  blob: number;
  recovery_envelope: number;
};

export type SiloReport = {
  app_version: string;
  name: string;
  path: string;
  silo_id: string | null;
  marker_version: number | null;
  formats: Formats;
  files: FileFact[];
  keys: KeyCounts | null;
  recovery_envelope: boolean;
  working_copy: FileFact | null;
  sync_provider: string | null;
  disk_free_bytes: number | null;
  disk_total_bytes: number | null;
};

/** UTC, and said so: two machines in different places would otherwise think
 *  they agree. */
export function reportTime(seconds: number | null): string {
  if (seconds === null) return "unknown";
  return `${new Date(seconds * 1000).toISOString().slice(0, 19).replace("T", " ")}Z`;
}

function fileLine(f: FileFact): string {
  if (!f.present) return `  ${f.name.padEnd(24)} missing`;
  return `  ${f.name.padEnd(24)} ${formatBytes(f.bytes ?? 0).padEnd(10)} ${reportTime(f.modified)}`;
}

function describeKeys(k: KeyCounts | null): string {
  if (k === null) return "cannot be read";
  if (k.total === 0) return "none enrolled";
  const parts = [`${k.platform} sealed to a machine`, `${k.portable} portable`];
  if (k.revoked > 0) parts.push(`${k.revoked} retired`);
  return `${k.total} enrolled (${parts.join(", ")})`;
}

function describeDisk(free: number | null, total: number | null): string {
  if (free === null || total === null) return "unknown";
  return `${formatBytes(free)} free of ${formatBytes(total)}`;
}

/** The whole report as text to paste into a message. Nothing is added here,
 *  so the rule about what is safe to show lives in one place. */
export function siloReportText(r: SiloReport): string {
  const f = r.formats;
  return [
    `SilentSilo ${r.app_version}`,
    `Silo        ${r.name}`,
    `Path        ${r.path}`,
    `Silo id     ${r.silo_id ?? "no marker on disk"}`,
    `Marker      ${r.marker_version === null ? "no marker on disk" : `version ${r.marker_version}`}`,
    "",
    "Formats this build writes",
    `  silo files ${f.silo_files} · marker ${f.marker} · index schema ${f.index_schema}`,
    `  sealed payload ${f.sealed_payload} · blob ${f.blob} · recovery envelope ${f.recovery_envelope}`,
    "",
    "Files",
    ...r.files.map(fileLine),
    "",
    `Keys          ${describeKeys(r.keys)}`,
    `Recovery      ${r.recovery_envelope ? "envelope present" : "no envelope"}`,
    `Working copy  ${
      r.working_copy === null
        ? "none, the last session closed cleanly"
        : `left behind, ${formatBytes(r.working_copy.bytes ?? 0)}, ${reportTime(r.working_copy.modified)}`
    }`,
    `Folder sync   ${r.sync_provider ?? "none detected"}`,
    `Disk          ${describeDisk(r.disk_free_bytes, r.disk_total_bytes)}`,
  ].join("\n");
}
