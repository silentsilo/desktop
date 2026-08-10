/**
 * Whether opening this file name hands the OS something that runs.
 *
 * Opening a file from the silo decrypts it and gives the path to the system
 * handler, which is what a file manager is expected to do. The difference
 * here is where the file came from: a silo syncs, so a document can arrive
 * from another device, and the person double-clicking it in a list of their
 * own documents is not thinking about what its extension does.
 *
 * A deliberately short list. It covers what runs on a double click with no
 * further prompt on Windows, plus the shell shortcut types that point
 * somewhere else entirely. It is not a malware filter and does not pretend to
 * be one: the answer is a confirmation, not a refusal, because these are the
 * user's own files.
 */
const RUNS_ON_OPEN = [
  // Programs and installers.
  "bat",
  "cmd",
  "com",
  "cpl",
  "exe",
  "msc",
  "msi",
  "msix",
  "appx",
  "msp",
  "mst",
  "pif",
  "scr",
  // Scripts. The `msh` family is the shell PowerShell grew out of, and
  // still opens.
  "hta",
  "jar",
  "js",
  "jse",
  "msh",
  "msh1",
  "msh2",
  "mshxml",
  "ps1",
  "psc1",
  "vb",
  "vbe",
  "vbs",
  "ws",
  "wsc",
  "wsf",
  "wsh",
  // Files whose whole purpose is to point at something else, which is what
  // makes them worth a question: what opens is not what the name describes.
  "appref-ms",
  "application",
  "inf",
  "library-ms",
  "lnk",
  "reg",
  "scf",
  "searchconnector-ms",
  "settingcontent-ms",
  "url",
  // Help and diagnostics, both of which execute and both of which have been
  // used to do so on purpose.
  "chm",
  "diagcab",
];

export function runsOnOpen(name: string): boolean {
  // Trailing dots and spaces are stripped by Windows before the extension is
  // resolved, so "payload.exe " opens the same program.
  const cleaned = name.trim().replace(/[. ]+$/, "");
  const dot = cleaned.lastIndexOf(".");
  if (dot <= 0) return false;
  return RUNS_ON_OPEN.includes(cleaned.slice(dot + 1).toLowerCase());
}
