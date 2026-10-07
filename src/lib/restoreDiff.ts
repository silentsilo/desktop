import { t } from "../i18n";
import { formatBytes } from "./format";

/**
 * A digest difference line, said in words.
 *
 * The backend compares two silos as canonical text lines, and those lines
 * are frozen: the compat fixtures diff the same format, so changing it there
 * is a format change. What lands on screen is this translation instead,
 * because "only here: file /Invoices::March.pdf deleted=false fav=0
 * size=88123 hash=ab mime=" is a debugging artefact, and the person reading
 * it has just been told their backup does not restore.
 *
 * Anything unrecognised is returned as it came: a raw line is still better
 * than a translation that guessed wrong.
 */
export function describeRestoreDifference(line: string): string {
  const sides: [string, string][] = [
    ["only here: ", t("files.diff_only_here")],
    ["only in the restore: ", t("files.diff_only_restore")],
  ];
  for (const [prefix, side] of sides) {
    if (line.startsWith(prefix)) {
      return describeDigestLine(line.slice(prefix.length), side) ?? line;
    }
  }
  return line;
}

function describeDigestLine(rest: string, side: string): string | null {
  const file = rest.match(
    /^file (.+) deleted=(true|false) fav=[01] size=(\d+) hash=\S* mime=\S*$/,
  );
  if (file) {
    const [, where, deleted, size] = file;
    // The digest writes "folderpath::name"; the screen writes a plain path.
    const [dir = "", name = ""] = where!.split("::");
    const path = dir === "/" ? `/${name}` : `${dir}/${name}`;
    return t(deleted === "true" ? "files.diff_file_trashed" : "files.diff_file", {
      side,
      path,
      size: formatBytes(Number(size)),
    });
  }

  const folder = rest.match(/^folder (.+) deleted=(true|false) fav=[01]$/);
  if (folder) {
    const [, path, deleted] = folder;
    return t(deleted === "true" ? "files.diff_folder_trashed" : "files.diff_folder", {
      side,
      path: path!,
    });
  }

  const password = rest.match(/^password (\S+)$/);
  if (password) {
    return t("files.diff_password", { side, id: password[1]!.slice(0, 8) });
  }

  const ops = rest.match(/^ops count=(\d+)$/);
  if (ops) {
    return t("files.diff_ops", { side, count: Number(ops[1]) });
  }

  return null;
}
