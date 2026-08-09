import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/?mock=unlocked");
  await page.getByRole("button", { name: "Settings" }).click();
});

test("the security panes carry their load-bearing content", async ({ page }) => {
  // Auto-lock is the landing pane.
  await expect(page.getByText("Lock", { exact: false }).first()).toBeVisible();

  await page.getByRole("button", { name: "Security keys" }).click();
  await expect(page.getByText("YubiKey 5C", { exact: true })).toBeVisible();
  await expect(page.getByText("Change the silo's encryption key")).toBeVisible();

  await page.getByRole("button", { name: "Recovery code" }).click();
  await expect(page.getByText("Print an emergency kit")).toBeVisible();
  // The kit preview is on the page, not behind the print dialog: the sheet
  // header is what proves it rendered.
  // Scoped to the on-screen preview: the printable copy is portalled under
  // <body> at zero size, which is invisible to a role query but not to this.
  await expect(page.locator(".kit-preview h1")).toHaveText("SilentSilo emergency kit");
});

test("the danger zone says what it deletes before offering to", async ({ page }) => {
  await page.getByRole("button", { name: "Danger zone" }).click();

  await expect(page.getByText("Removing Personal from this computer")).toBeVisible();
  await expect(page.getByRole("button", { name: "Remove this silo" })).toBeVisible();
});
