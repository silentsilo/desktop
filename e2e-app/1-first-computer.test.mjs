// The first computer: a silo made, its key enrolled, a recovery code written
// down, backup storage on a folder, a file added and backed up, the backup
// tested both ways, then a lock and an unlock with the file still there.
// Leaves the storage and the code for the second computer.

import { after, before, test } from "node:test";
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { By, until } from "selenium-webdriver";

import { answerNextDialog, click, launch, sees, snapshot, type } from "./app.mjs";

const shared = process.env.SILENTSILO_E2E_SHARED;
let driver;
before(async () => {
  driver = await launch();
});
after(async () => {
  if (driver) console.log(`last screen: ${await snapshot(driver, "last")}`);
  await driver?.quit();
});

test("a new silo is backed up, passes its backup test and survives a lock", async () => {
  await sees(driver, "New silo");
  await click(driver, "Create silo");
  await sees(driver, "Set up unlocking");
  await click(driver, "Use a security key");
  await sees(driver, "Security key enrolled. It opens this silo from now on.");

  await click(driver, "Create a recovery code");
  const shown = await driver.wait(until.elementLocated(By.css("code.recovery-code")), 20_000);
  const code = await shown.getText();
  writeFileSync(join(shared, "code.txt"), code);
  await click(driver, "I've written it down");

  await sees(driver, "Backup storage");
  await click(driver, "Set up backup storage");
  await click(driver, "A drive or NAS folder");
  const storage = join(shared, "storage");
  mkdirSync(storage);
  await answerNextDialog(driver, storage);
  await click(driver, "Browse");
  await click(driver, "Save and connect");
  await sees(driver, "Sync now");

  await click(driver, "Files");
  const source = join(process.env.SILENTSILO_E2E_DIR, "tax return 2025.pdf");
  writeFileSync(source, "not really a pdf");
  await answerNextDialog(driver, [source]);
  await click(driver, "Add");
  await click(driver, "Add files");
  await sees(driver, "tax return 2025.pdf");

  await click(driver, "Settings");
  await click(driver, "Backup");
  await click(driver, "Sync now");
  await click(driver, "Test backup");
  await click(driver, "Read every file back");
  await sees(driver, "No problems found.");
  await type(driver, "input", code);
  await click(driver, "Try a recovery now");
  await sees(driver, "Recovery works.");

  await click(driver, "Lock silo");
  await click(driver, "Unlock");
  await click(driver, "Files");
  await sees(driver, "tax return 2025.pdf");
});
