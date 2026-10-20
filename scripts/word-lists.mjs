/**
 * Writes the two lists the app carries inside it, from files downloaded by
 * hand, so the app itself never fetches either:
 *
 *   node scripts/word-lists.mjs <eff_large_wordlist.txt> <totp.json>
 *
 * - src/lib/wordlist.ts: the EFF large word list (7,776 words, CC BY 3.0 US,
 *   https://www.eff.org/dice), for passphrases and usernames.
 * - src/lib/twoFactorSites.ts: the domains 2fa.directory lists as offering
 *   TOTP codes (MIT, https://api.2fa.directory/v3/totp.json), for Health.
 *
 * Refresh the second before a release. The mobile app keeps copies of both
 * files (`src/shared/`); copy them over after running this.
 */
import { readFileSync, writeFileSync } from "node:fs";

const [effPath, totpPath] = process.argv.slice(2);
if (!effPath || !totpPath) {
  console.error("usage: node scripts/word-lists.mjs <eff_large_wordlist.txt> <totp.json>");
  process.exit(1);
}

const words = readFileSync(effPath, "utf8")
  .split(/\r?\n/)
  .filter((line) => line.trim())
  .map((line) => line.split("\t")[1].trim());
if (words.length !== 7776 || new Set(words).size !== 7776 || !words.every((w) => /^[a-z-]+$/.test(w))) {
  throw new Error("not the EFF large word list: expected 7,776 distinct lowercase words");
}

const sites = new Set();
for (const [, info] of JSON.parse(readFileSync(totpPath, "utf8"))) {
  for (const domain of [info.domain, ...(info["additional-domains"] ?? [])]) {
    if (typeof domain === "string" && /^[a-z0-9.-]+$/i.test(domain)) sites.add(domain.toLowerCase());
  }
}
if (sites.size < 500) throw new Error(`only ${sites.size} domains: not the 2fa.directory TOTP list`);

/** One long string split at runtime: a fraction of the size of an array literal. */
function chunked(list) {
  const out = [];
  let line = "";
  for (const item of list) {
    if (line.length + item.length > 90) {
      out.push(line);
      line = "";
    }
    line += (line ? " " : "") + item;
  }
  if (line) out.push(line);
  return out.map((l) => `  "${l}",`).join("\n");
}

writeFileSync(
  "src/lib/wordlist.ts",
  `/**
 * The EFF large word list for passphrases: 7,776 words, so each word picked
 * at random adds about 12.9 bits. By the Electronic Frontier Foundation,
 * https://www.eff.org/dice, under CC BY 3.0 US. Written by
 * scripts/word-lists.mjs; do not edit by hand.
 */
export const WORDS: readonly string[] = [
${chunked(words)}
]
  .join(" ")
  .split(" ");
`,
);

writeFileSync(
  "src/lib/twoFactorSites.ts",
  `/**
 * Sites that offer two-factor sign-in with a code from an authenticator app
 * (TOTP), by domain: ${sites.size} domains from 2fa.directory, MIT,
 * https://2fa.directory. Carried inside the app, never fetched. Written by
 * scripts/word-lists.mjs; do not edit by hand.
 */
export const TOTP_SITES: ReadonlySet<string> = new Set(
  [
${chunked([...sites].sort()).replace(/^ {2}/gm, "    ")}
  ]
    .join(" ")
    .split(" "),
);
`,
);
console.log(`${words.length} words, ${sites.size} domains`);
