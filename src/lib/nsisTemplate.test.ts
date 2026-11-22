import { describe, expect, it } from "vitest";
import fs from "node:fs";
import path from "node:path";

// The Windows installer is built from our copy of Tauri's NSIS template,
// with one change: an upgrade installs over the installed copy without
// offering to uninstall it first (src-tauri/nsis/installer.nsi). A template from
// one CLI version under another can break the installer, so a CLI upgrade
// must bring a fresh copy of the template with the change made again.

const root = path.resolve(__dirname, "../..");
const template = fs.readFileSync(path.join(root, "src-tauri/nsis/installer.nsi"), "utf8");
const cli = JSON.parse(
  fs.readFileSync(path.join(root, "node_modules/@tauri-apps/cli/package.json"), "utf8"),
) as { version: string };
const conf = JSON.parse(fs.readFileSync(path.join(root, "src-tauri/tauri.conf.json"), "utf8")) as {
  bundle: { windows: { nsis: { template?: string } } };
};

describe("the NSIS template", () => {
  it("is the one for the Tauri CLI that builds the installer", () => {
    expect(template).toContain(`from tauri-cli-v${cli.version}`);
  });

  it("is the one the build uses", () => {
    expect(conf.bundle.windows.nsis.template).toBe("nsis/installer.nsi");
  });

  it("installs over the copy it finds on an upgrade, without the uninstall page", () => {
    expect(template).toMatch(
      /; SilentSilo:[\s\S]*?\$\{If\} \$R0 = 1\s+\$\{AndIf\} \$WixMode = 0\s+Abort\s+\$\{EndIf\}\s+; Skip showing the page if passive/,
    );
  });
});
