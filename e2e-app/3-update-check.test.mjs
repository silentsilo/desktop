// The update check against a server on this machine (the e2e build asks
// SILENTSILO_E2E_LATEST_JSON instead of the site): a newer version shows the
// card with its two buttons, the request says nothing about the install,
// "Postpone 1 week" holds when the window loads again, and an answer that is not a
// plain version, or too large, shows nothing. "Get latest version" is not
// pressed: it opens the real browser. Its address is fixed in the app and
// covered by the unit tests.

import { after, test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { By } from "selenium-webdriver";

import { launch, sees, snapshot, click } from "./app.mjs";

const PORT = Number(process.env.SILENTSILO_E2E_LATEST_JSON.match(/:(\d+)\//)[1]);
const requests = [];
let answer = '{"version":"99.0.0"}';
const server = createServer((req, res) => {
  requests.push({ url: req.url, method: req.method, headers: req.headers });
  res.writeHead(200, { "Content-Type": "application/json" });
  res.end(answer);
});
await new Promise((up) => server.listen(PORT, "127.0.0.1", up));

let driver;
after(async () => {
  if (driver) console.log(`last screen: ${await snapshot(driver, "last")}`);
  await driver?.quit();
  server.close();
});

/** A fresh start. The driver gives each session a WebView2 profile of its
 *  own, so storage starts empty. */
const restart = async () => {
  await driver?.quit();
  driver = await launch();
  await sees(driver, "New silo");
};

/** The window loaded again in the same process, storage kept: what the app
 *  reads at start-up, without a new profile. */
const reload = async () => {
  await driver.navigate().refresh();
  await sees(driver, "New silo");
};

const cardShown = async () =>
  (await driver.findElements(By.xpath("//button[normalize-space(.)='Postpone 1 week']"))).length > 0;

/** Lets a check that may be under way finish. */
const settle = () => new Promise((done) => setTimeout(done, 3000));

/** Makes the next start ask again, as a day passing would. */
const forgetLastCheck = () =>
  driver.executeScript(`localStorage.removeItem("silentsilo.update.lastCheckAt");`);

test("a newer version shows the card, and the request carries nothing", async () => {
  driver = await launch();
  await sees(driver, "Get latest version");
  assert.ok(await cardShown());
  assert.equal(requests.length, 1, "one request per check");
  const [first] = requests;
  assert.equal(first.method, "GET");
  assert.equal(first.url, "/latest.json", "no query string");
  assert.equal(first.headers["user-agent"], "SilentSilo", "no version in the agent");
  assert.equal(first.headers.cookie, undefined);
  assert.equal(first.headers.authorization, undefined);
});

test("postpone hides that version, also after a reload, and sends nothing more that day", async () => {
  await click(driver, "Postpone 1 week");
  await settle();
  assert.ok(!(await cardShown()), "the card stayed after postpone");
  await reload();
  await settle();
  assert.ok(!(await cardShown()), "the card came back after a reload");
  assert.equal(requests.length, 1, "a reload inside the day asked again");
});

test("a newer version than the one postponed shows at once", async () => {
  answer = '{"version":"99.0.1"}';
  await forgetLastCheck();
  await reload();
  await sees(driver, "Get latest version");
  const postponed = await driver.executeScript(
    `return localStorage.getItem("silentsilo.update.postponed")`,
  );
  assert.match(postponed, /"99\.0\.0"/, "99.0.0 is still the one postponed");
  assert.ok(await cardShown());
});

test("an answer that is not a plain version, or too large, shows nothing", async () => {
  for (const bad of [
    '{"version":"999.0.0+ reinstall from evil.example"}',
    '{"version":"999.0.0 <b>x</b>"}',
    `{"version":"999.0.0","pad":"${"x".repeat(5000)}"}`,
    "<html>not json</html>",
    '{"version":"1.0.0"}',
  ]) {
    answer = bad;
    await driver.executeScript(
      `localStorage.removeItem("silentsilo.update.lastCheckAt");
       localStorage.removeItem("silentsilo.update.pendingVersion");`,
    );
    await restart();
    await settle();
    assert.ok(!(await cardShown()), `a card for ${bad.slice(0, 60)}`);
  }
});
