import { test, expect } from "@playwright/test";

test("the picker lists silos and says which cannot open", async ({ page }) => {
  await page.goto("/?mock=picker");

  await expect(page.getByRole("heading", { name: "Your silos" })).toBeVisible();
  // The badge reads OPEN on screen; the uppercasing is CSS, the text is not.
  await expect(page.locator(".silo-open-badge")).toHaveText("open");
  await expect(page.getByText("not reachable", { exact: false }).first()).toBeVisible();
  // The explanation for the unreachable state, so the row does not read as
  // a silo that fails for invisible reasons.
  await expect(page.getByText("Plug it back in", { exact: false })).toBeVisible();
});

test("the theme toggle flips the whole surface", async ({ page }) => {
  await page.goto("/?mock=picker");

  // colorScheme is dark in the config, so light is the change to look for.
  await expect(page.locator(".light-theme")).toHaveCount(0);
  await page.getByRole("button", { name: "Toggle theme" }).click();
  await expect(page.locator(".light-theme").first()).toBeVisible();
});
