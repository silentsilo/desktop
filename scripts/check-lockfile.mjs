// The core crates must come from silentsilo/core by git, never from a local
// path. A `[patch]` in .cargo/config.toml is the normal way to work on both
// repositories at once, and it rewrites Cargo.lock: the patched crate loses
// its `source` line entirely and looks like a workspace member. Committed,
// that lockfile builds the app against whatever happens to sit in a sibling
// directory, which on the release machine is not the tagged code.
//
// `--locked` already refuses to proceed on a lockfile that disagrees with the
// manifest, but it fails with a message about updating the lock rather than
// about what actually happened. This says what happened.
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const CORE = [
  "silentsilo-core",
  "silentsilo-crypto",
  "silentsilo-vault",
  "silentsilo-vfs",
  "silentsilo-store",
  "silentsilo-sync",
  "silentsilo-fido",
  "silentsilo-s3",
];

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const lock = readFileSync(path.join(root, "Cargo.lock"), "utf8");
const bad = [];
const seen = new Set();

for (const block of lock.split("[[package]]").slice(1)) {
  const name = /^name = "(.+)"$/m.exec(block)?.[1];
  if (!CORE.includes(name)) continue;
  seen.add(name);
  const source = /^source = "(.+)"$/m.exec(block)?.[1];
  if (!source?.startsWith("git+https://github.com/silentsilo/core")) {
    bad.push(`${name}: ${source ?? "no source (a local path or a patch)"}`);
  }
}

const missing = CORE.filter((c) => !seen.has(c));
if (missing.length) bad.push(`not in the lockfile at all: ${missing.join(", ")}`);

if (bad.length) {
  console.error("Cargo.lock does not point at silentsilo/core:\n  " + bad.join("\n  "));
  console.error("\nRemove .cargo/config.toml and run `cargo check` to restore it.");
  process.exit(1);
}

console.log(`Cargo.lock: all ${CORE.length} core crates come from silentsilo/core.`);
