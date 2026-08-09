/**
 * Writes the screenshots the README shows, into docs/screenshots/.
 *
 * Deliberately not a test. The baselines under e2e/ exist to fail when the
 * UI changes, and they are composed to stress layout: a banner in the way, a
 * file name long enough to truncate. These are composed to be looked at, so
 * pointing the README at a baseline would make every deliberate UI change a
 * choice between a red test and a stale picture.
 *
 * Needs the dev server running, because it drives the mock backend:
 *
 *   npm run dev
 *   node scripts/screenshots.mjs
 *
 * Override the address with SILENTSILO_URL when the dev server is not on
 * http://localhost:1420 (Vite binds to ::1 on some Windows machines, where
 * Chromium resolves localhost to 127.0.0.1 and cannot reach it).
 */
import { chromium } from "@playwright/test";
import { mkdir } from "node:fs/promises";
import { join } from "node:path";

const base = process.env.SILENTSILO_URL ?? "http://localhost:1420";
const out = join(import.meta.dirname, "..", "docs", "screenshots");

// Evaluated inside the page, where `document` is the browser's own. ESLint
// reads this file as Node, which is what it is everywhere else.
// eslint-disable-next-line no-undef
const fontsReady = () => document.fonts.ready;

/** Fonts arrive after first paint, and a screenshot before them is blurry. */
async function settle(page, url, anchor) {
  await page.goto(`${base}${url}`);
  await page.getByText(anchor).first().waitFor({ state: "visible" });
  await page.evaluate(fontsReady);
  await page.waitForTimeout(150);
}

const shots = [
  {
    name: "unlock",
    async take(page) {
      await settle(page, "/?mock=unlock", "Security key ready");
    },
  },
  {
    name: "files",
    async take(page) {
      await settle(page, "/?mock=unlocked", "Passport scan.pdf");
      // The offline-content banner is a real state worth testing and the
      // wrong thing to greet a reader with.
      await page.getByRole("button", { name: "Not now" }).click();
    },
  },
  {
    name: "credentials",
    async take(page) {
      await settle(page, "/?mock=unlocked", "Passport scan.pdf");
      await page.getByRole("button", { name: "Credentials" }).click();
      await page.getByText("Bank", { exact: true }).first().click();
      await page.getByText("Branch phone: 021 000 000").waitFor({ state: "visible" });
    },
  },
  {
    name: "backup",
    async take(page) {
      await settle(page, "/?mock=unlocked", "Passport scan.pdf");
      await page.getByRole("button", { name: "Settings" }).click();
      await page.getByRole("button", { name: "Backup" }).click();
    },
  },
  {
    name: "kit",
    async take(page) {
      await settle(page, "/?mock=unlocked", "Passport scan.pdf");
      await page.getByRole("button", { name: "Settings" }).click();
      await page.getByRole("button", { name: "Recovery code" }).click();
      // The sheet is the point of this screen, and it sits below the fold.
      // Filled in rather than blank: empty boxes photograph as a form, and
      // what is worth showing is the paper someone files away.
      await page.getByRole("button", { name: "Print the code too" }).click();
      await page
        .getByPlaceholder("Type it from your existing sheet")
        .fill("K7QM-3PXR-9TWD-2FHN-6BJL-8VCY-4SGZ-5RKA");
      // Two sheets are in the DOM, the on-screen preview and the one the
      // print stylesheet uses, so this takes the first rather than a text
      // match that would resolve to both.
      await page.locator(".kit-sheet").first().scrollIntoViewIfNeeded();
    },
  },
  {
    name: "silos",
    async take(page) {
      await settle(page, "/?mock=picker", "Your silos");
    },
  },
  {
    name: "restore",
    async take(page) {
      await settle(page, "/?mock=picker", "Your silos");
      await page.getByRole("button", { name: "Copy one from backup storage" }).click();
    },
  },
];

const browser = await chromium.launch();
const page = await browser.newPage({
  viewport: { width: 1200, height: 800 },
  colorScheme: "dark",
  deviceScaleFactor: 2,
});

// A cold dev server pre-bundles its dependencies on the first request that
// pulls the whole module graph, which a browser does and a curl does not.
// The default 30s is short enough to fail only on the first run of the day.
page.setDefaultNavigationTimeout(120_000);

await mkdir(out, { recursive: true });
for (const shot of shots) {
  await shot.take(page);
  await page.evaluate(fontsReady);
  // Panels fade in. Catching one halfway leaves half the screen greyed, which
  // reads as a disabled section rather than as a shot taken too early.
  await page.waitForTimeout(700);
  await page.screenshot({ path: join(out, `${shot.name}.png`) });
  console.log(`wrote docs/screenshots/${shot.name}.png`);
}

await browser.close();
