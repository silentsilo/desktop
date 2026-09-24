// The second computer: nothing on it yet. It finds the silo the first one
// backed up, sets it up with the same key, and shows the file.

import { after, before, test } from "node:test";
import { join } from "node:path";

import { answerNextDialog, click, launch, sees, snapshot } from "./app.mjs";

const shared = process.env.SILENTSILO_E2E_SHARED;
let driver;
before(async () => {
  driver = await launch();
});
after(async () => {
  if (driver) console.log(`last screen: ${await snapshot(driver, "last")}`);
  await driver?.quit();
});

test("a second computer sets the silo up from backup storage with the key", async () => {
  await sees(driver, "New silo");
  await click(driver, "Set up from backup storage");
  await click(driver, "A drive or NAS folder");
  await answerNextDialog(driver, join(shared, "storage"));
  await click(driver, "Browse");
  await click(driver, "See what is there");
  await sees(driver, "Found a silo");
  await click(driver, "Set up on this computer");
  await sees(driver, "tax return 2025.pdf");
});
