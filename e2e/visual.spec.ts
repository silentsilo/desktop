import { test, expect, type Page } from "@playwright/test";

/**
 * Screenshot baselines for the screens people live in, both themes where
 * the surface differs. What these catch is the class of defect no flow
 * test sees and no unit test can: a date folding onto two lines, a card
 * taller than its neighbours, a theme that misses one panel.
 *
 * Gated: baselines are rendered by one platform's font stack, so they only
 * compare cleanly where they were made. Run with SILENTSILO_VISUAL=1 on
 * the platform whose baselines are committed, and regenerate deliberately
 * with --update-snapshots when a screen is meant to change.
 */
test.skip(
  process.env.SILENTSILO_VISUAL !== "1",
  "visual baselines are platform-bound; set SILENTSILO_VISUAL=1 to compare",
);

async function settled(page: Page, url: string, anchor: string) {
  await page.goto(url);
  await expect(page.getByText(anchor).first()).toBeVisible();
  // Fonts arrive after first paint; a screenshot before them diffs forever.
  await page.evaluate(() => document.fonts.ready);
}

/**
 * A mask is only as stable as the box it covers, and the sidebar version is
 * sized by its own text: "1.0.0" is narrower than "0.1.0-rc.11", so the
 * masked rectangle changed shape on the 1.0.0 bump and took every baseline
 * with a sidebar down with it. Stamping a fixed string first gives the mask
 * the same box on every release, which is what it was meant to buy.
 */
async function freezeVersion(page: Page) {
  await page.evaluate(() => {
    const el = document.querySelector(".sidebar-version");
    if (el) el.textContent = "v0.0.0";
  });
}

const screens: Array<{ name: string; url: string; anchor: string }> = [
  { name: "picker", url: "/?mock=picker", anchor: "Your silos" },
  { name: "unlock", url: "/?mock=unlock", anchor: "Security key ready" },
  { name: "files-grid", url: "/?mock=unlocked", anchor: "Passport scan.pdf" },
];

for (const screen of screens) {
  for (const scheme of ["dark", "light"] as const) {
    test(`${screen.name}, ${scheme}`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await settled(page, screen.url, screen.anchor);
      await freezeVersion(page);
      await expect(page).toHaveScreenshot(`${screen.name}-${scheme}.png`, {
        fullPage: false,
        // The sidebar version would fail every baseline on every release
        // bump, which would teach people to regenerate without looking.
        mask: [page.locator(".sidebar-version")],
      });
    });
  }
}

test("enrolment, dark", async ({ page }) => {
  await page.goto("/?mock=empty");
  await page.getByRole("button", { name: "Create silo" }).click();
  await expect(page.getByRole("heading", { name: "Set up unlocking" })).toBeVisible();
  await page.evaluate(() => document.fonts.ready);
  await freezeVersion(page);
  await expect(page).toHaveScreenshot("enroll-dark.png", {
    mask: [page.locator(".sidebar-version")],
  });
});

test("credentials with a selected entry, dark", async ({ page }) => {
  await settled(page, "/?mock=unlocked", "Passport scan.pdf");
  await page.getByRole("button", { name: "Credentials" }).click();
  await page.getByText("Bank", { exact: true }).first().click();
  await expect(page.getByText("Branch phone: 021 000 000")).toBeVisible();
  await freezeVersion(page);
  await expect(page).toHaveScreenshot("credentials-dark.png", {
    mask: [page.locator(".sidebar-version")],
  });
});

test("security keys settings, dark", async ({ page }) => {
  await settled(page, "/?mock=unlocked", "Passport scan.pdf");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("button", { name: "Security keys" }).click();
  await expect(page.getByText("YubiKey 5C", { exact: true })).toBeVisible();
  await freezeVersion(page);
  await expect(page).toHaveScreenshot("settings-keys-dark.png", {
    mask: [page.locator(".sidebar-version")],
  });
});
