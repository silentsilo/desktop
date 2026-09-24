// What the end-to-end tests share: a session with the real app, and ways to
// find things the way the Playwright tests do, by what the user reads.

import { Builder, By, until } from "selenium-webdriver";

const WAIT = 20_000;

export async function launch() {
  return new Builder()
    .usingServer("http://127.0.0.1:4444/")
    .withCapabilities({
      browserName: "wry",
      "tauri:options": { application: process.env.SILENTSILO_E2E_APP },
    })
    .build();
}

const literal = (text) =>
  text.includes("'") ? `concat('${text.split("'").join(`', "'", '`)}')` : `'${text}'`;

/** A button by its text or its accessible name. */
export async function button(driver, name) {
  const xpath = `//*[(self::button or @role='button' or @role='menuitem') and (normalize-space(.)=${literal(name)} or @aria-label=${literal(name)})]`;
  const found = await driver.wait(until.elementLocated(By.xpath(xpath)), WAIT, `no button "${name}"`);
  await driver.wait(until.elementIsEnabled(found), WAIT, `"${name}" stays disabled`);
  return found;
}

/** Clicks, waiting out a toast or overlay that sits over the button. */
export async function click(driver, name) {
  const deadline = Date.now() + WAIT;
  for (;;) {
    try {
      await (await button(driver, name)).click();
      return;
    } catch (e) {
      const covered = e.name === "ElementClickInterceptedError" || e.name === "StaleElementReferenceError";
      if (!covered || Date.now() > deadline) throw e;
      await new Promise((r) => setTimeout(r, 250));
    }
  }
}

/** A screenshot next to the test files, for a failure to be read. */
export async function snapshot(driver, name) {
  const { writeFileSync } = await import("node:fs");
  const { join } = await import("node:path");
  const path = join(process.env.SILENTSILO_E2E_DIR, `${name}.png`);
  writeFileSync(path, Buffer.from(await driver.takeScreenshot(), "base64"));
  return path;
}

/** Types into the first visible field matching `css`. */
export async function type(driver, css, text) {
  const field = await driver.wait(until.elementLocated(By.css(css)), WAIT, `no field ${css}`);
  await field.sendKeys(text);
}

/** Waits until the page shows `text` somewhere. */
export async function sees(driver, text) {
  const xpath = `//*[contains(normalize-space(.), ${literal(text)})]`;
  await driver.wait(until.elementLocated(By.xpath(xpath)), WAIT, `never showed "${text}"`);
}

/**
 * The next file dialog answers `value` instead of opening: a runner cannot
 * press a native dialog. See `src/lib/dialog.ts`.
 */
export async function answerNextDialog(driver, value) {
  await driver.executeScript(
    `(window.__silentsiloDialogAnswers ??= []).push(arguments[0]);`,
    value,
  );
}
