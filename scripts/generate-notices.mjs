/**
 * Builds THIRD-PARTY-NOTICES.txt from the resolved dependency graph, reading
 * licence files off this disk so the notices match what was built. MIT, BSD
 * and Apache all require their notices to travel with the binary.
 *
 * Run with: npm run notices
 */

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const TARGET = "x86_64-pc-windows-msvc";

/** Our own crates and the app itself: the AGPL covers these. */
const OURS = /^silentsilo(-|$)/;

/** What a licence file is called, in rough order of how likely it is. */
const LICENCE_FILE = /^(LICENSE|LICENCE|COPYING|NOTICE|UNLICENSE)([-.].*)?$/i;

function licenceTexts(dir) {
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  return entries
    .filter((e) => e.isFile() && LICENCE_FILE.test(e.name))
    .map((e) => {
      try {
        return { name: e.name, text: fs.readFileSync(path.join(dir, e.name), "utf8").trim() };
      } catch {
        return null;
      }
    })
    .filter((t) => t && t.text.length > 0);
}

function rustComponents() {
  const raw = execFileSync(
    "cargo",
    ["metadata", "--format-version", "1", "--filter-platform", TARGET],
    { cwd: root, encoding: "utf8", maxBuffer: 128 * 1024 * 1024 }
  );
  return JSON.parse(raw)
    .packages.filter((p) => !OURS.test(p.name))
    .map((p) => ({
      kind: "crate",
      name: p.name,
      version: p.version,
      licence: p.license ?? "see the text below",
      repository: p.repository ?? "",
      texts: licenceTexts(path.dirname(p.manifest_path)),
    }));
}

/** Frontend packages whose code is in the bundle. Dev tooling is left out:
 *  a linter that never ships imposes nothing on anyone. */
function nodeComponents() {
  const manifest = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
  const wanted = new Set(Object.keys(manifest.dependencies ?? {}));
  const seen = new Map();

  const walk = (name) => {
    if (seen.has(name)) return;
    const dir = path.join(root, "node_modules", ...name.split("/"));
    let pkg;
    try {
      pkg = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
    } catch {
      return;
    }
    seen.set(name, {
      kind: "npm package",
      name,
      version: pkg.version ?? "",
      licence: typeof pkg.license === "string" ? pkg.license : "see the text below",
      repository: typeof pkg.repository === "string" ? pkg.repository : (pkg.repository?.url ?? ""),
      texts: licenceTexts(dir),
    });
    // Transitive: what a shipped package pulls in ships with it.
    for (const dep of Object.keys(pkg.dependencies ?? {})) walk(dep);
  };

  for (const name of wanted) walk(name);
  return [...seen.values()];
}

const components = [...rustComponents(), ...nodeComponents()].sort((a, b) =>
  a.name.localeCompare(b.name)
);

// One copy of each distinct text, referenced by number: otherwise the file is
// megabytes of the same MIT paragraph, hiding the copyright lines.
const bodies = new Map();
for (const c of components) {
  c.refs = c.texts.map(({ name, text }) => {
    const key = createHash("sha256").update(text).digest("hex");
    if (!bodies.has(key)) bodies.set(key, { index: bodies.size + 1, name, text });
    return bodies.get(key).index;
  });
}

/** Standard text for the few components that declare a licence and ship no
 *  file. MIT and BSD carry a note, since their copyright line is not ours to
 *  invent. */
const STANDARD = {
  MIT: `The MIT License

(This component declares MIT and ships no licence file. The standard terms
follow; the copyright is held by that component's authors, named at its
source repository above.)

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.`,
  "BSD-3-Clause": `The 3-Clause BSD License

(This component declares BSD-3-Clause and ships no licence file. The standard
terms follow; the copyright is held by that component's authors, named at its
source repository above.)

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice,
   this list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors
   may be used to endorse or promote products derived from this software
   without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE
LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
CONSEQUENTIAL DAMAGES HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER
IN CONTRACT, STRICT LIABILITY, OR TORT, ARISING IN ANY WAY OUT OF THE USE OF
THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.`,
};

/** Licences whose text is the same wherever it appears, so a copy already
 *  collected from another component is the genuine article. */
const INVARIANT = [
  ["Apache-2.0", "Apache License"],
  ["MPL-2.0", "Mozilla Public License Version 2.0"],
  ["CC0-1.0", "CC0 1.0 Universal"],
  ["Unlicense", "This is free and unencumbered software"],
  ["Unicode-3.0", "UNICODE LICENSE"],
];

function fillMissing() {
  const byPhrase = (phrase) =>
    [...bodies.values()].find((b) => b.text.includes(phrase))?.index ?? null;

  for (const c of components) {
    if (c.refs.length) continue;
    const declared = (c.licence || "").toUpperCase();

    for (const [id, phrase] of INVARIANT) {
      if (declared.includes(id.toUpperCase())) {
        const at = byPhrase(phrase);
        if (at) {
          c.refs.push(at);
          c.note = `declares ${id} and ships no file; the standard text is at [${at}]`;
          break;
        }
      }
    }
    if (c.refs.length) continue;

    for (const id of Object.keys(STANDARD)) {
      if (declared.includes(id.toUpperCase())) {
        const key = createHash("sha256").update(STANDARD[id]).digest("hex");
        if (!bodies.has(key))
          bodies.set(key, { index: bodies.size + 1, name: `${id} (standard text)`, text: STANDARD[id] });
        const at = bodies.get(key).index;
        c.refs.push(at);
        c.note = `declares ${id} and ships no file; the standard text is at [${at}]`;
        break;
      }
    }
  }
}
fillMissing();

const stamp = new Date().toISOString().slice(0, 10);
const out = [];
out.push("Third-party notices for SilentSilo");
out.push("");
out.push(
  `SilentSilo itself is licensed under the GNU Affero General Public License,`,
  `version 3 or later. This file is about everything else it is built from.`,
  ``,
  `The components below are linked into the application or bundled with it.`,
  `Each is listed with its version and its licence, and the full text of every`,
  `licence follows, once per distinct text. Where a component ships its own`,
  `copyright or NOTICE file, that file is what is reproduced here.`,
  ``,
  `Generated from the resolved dependency graph on ${stamp}, for ${TARGET}.`,
  `Regenerate with: npm run notices`,
  ``,
  `Components: ${components.length}`,
  ``,
  "=".repeat(78),
  "COMPONENTS",
  "=".repeat(78),
  ""
);

for (const c of components) {
  const where = c.refs.length ? `see ${c.refs.map((i) => `[${i}]`).join(" ")}` : "no text found";
  out.push(`${c.name} ${c.version} (${c.kind})`);
  out.push(`  licence: ${c.licence}`);
  if (c.repository) out.push(`  source:  ${c.repository}`);
  out.push(`  text:    ${c.note ?? where}`);
  out.push("");
}

out.push("=".repeat(78), "LICENCE TEXTS", "=".repeat(78), "");
for (const { index, name, text } of [...bodies.values()].sort((a, b) => a.index - b.index)) {
  out.push(`[${index}] ${name}`, "-".repeat(78), text, "");
}

const dest = path.join(root, "THIRD-PARTY-NOTICES.txt");
fs.writeFileSync(dest, out.join("\n"), "utf8");

const missing = components.filter((c) => c.refs.length === 0);
console.log(`Wrote ${path.relative(root, dest)}`);
console.log(`  components:     ${components.length}`);
console.log(`  licence texts:  ${bodies.size}`);
console.log(`  size:           ${(fs.statSync(dest).size / 1024).toFixed(0)} KB`);
if (missing.length) {
  console.log(`  no text shipped: ${missing.length}`);
  for (const c of missing.slice(0, 20)) console.log(`    ${c.name} ${c.version} (${c.licence})`);
  if (missing.length > 20) console.log(`    ... and ${missing.length - 20} more`);
}
