// Runs the end-to-end tests against the real app: the e2e build, driven over
// WebDriver by tauri-driver.
//
//   npm run build
//   cargo build -p silentsilo --features e2e   (CARGO_TARGET_DIR=target/e2e)
//   npm run test:app
//
// Each test file is one computer: its own tauri-driver and its own folder
// for everything the app writes, in file-name order. What they share is
// SILENTSILO_E2E_SHARED: the backup storage and a recovery code, so a later
// file can join the silo an earlier one made.
//
// MSEDGEDRIVER names msedgedriver.exe, the version of the installed WebView2;
// by default ../tools/msedgedriver/msedgedriver.exe beside the repositories.

import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const app = resolve(root, "target/e2e/debug/silentsilo.exe");
const edge =
  process.env.MSEDGEDRIVER ?? resolve(root, "../tools/msedgedriver/msedgedriver.exe");
for (const [what, path] of [
  ["the e2e build", app],
  ["msedgedriver", edge],
]) {
  if (!existsSync(path)) {
    console.error(`${what} is not at ${path}`);
    process.exit(2);
  }
}

const run = mkdtempSync(join(tmpdir(), "silentsilo-e2e-"));
const shared = join(run, "shared");
mkdirSync(shared);
console.log(`files under ${run}`);

const files = readdirSync(join(root, "e2e-app"))
  .filter((f) => f.endsWith(".test.mjs"))
  .sort();
let failed = false;
for (const file of files) {
  const dir = join(run, file.replace(".test.mjs", ""));
  mkdirSync(dir);
  const env = {
    ...process.env,
    SILENTSILO_E2E_DIR: dir,
    SILENTSILO_E2E_SHARED: shared,
    SILENTSILO_E2E_APP: app,
  };
  const driver = spawn("tauri-driver", ["--native-driver", edge], { env, stdio: "inherit" });
  await new Promise((ready) => setTimeout(ready, 1500));
  const test = spawn(process.execPath, ["--test", join("e2e-app", file)], {
    cwd: root,
    env,
    stdio: "inherit",
  });
  const code = await new Promise((done) => test.on("exit", done));
  driver.kill();
  await new Promise((gone) => driver.on("exit", gone));
  if (code !== 0) {
    failed = true;
    break;
  }
}
process.exit(failed ? 1 : 0);
