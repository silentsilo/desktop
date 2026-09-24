import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/?mock=unlocked");
  await page.getByRole("button", { name: "Settings" }).click();
});

test("settings open on an overview of what keeps the silo safe", async ({ page }) => {
  const pane = page.locator(".settings-pane");
  await expect(pane.getByText("What keeps this silo safe.")).toBeVisible();
  await expect(pane.getByText("Recovery code", { exact: true })).toBeVisible();
  await expect(pane.getByText("A key you can carry", { exact: true })).toBeVisible();
});

test("the security panes carry their load-bearing content", async ({ page }) => {
  await page.getByRole("button", { name: "Unlocking", exact: true }).click();
  await expect(page.getByText("YubiKey 5C", { exact: true })).toBeVisible();
  await expect(page.getByText("Lock", { exact: false }).first()).toBeVisible();

  // The rare, irreversible actions live apart from the everyday ones.
  await page.getByRole("button", { name: "Advanced", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Replace the encryption key" })).toBeVisible();

  await page.getByRole("button", { name: "Recovery code", exact: true }).click();
  await expect(page.getByText("Print an emergency kit")).toBeVisible();
  // The kit preview is on the page, not behind the print dialog: the sheet
  // header is what proves it rendered.
  // Scoped to the on-screen preview: the printable copy is portalled under
  // <body> at zero size, which is invisible to a role query but not to this.
  await expect(page.locator(".kit-preview h1")).toHaveText("SilentSilo emergency kit");
});

test("removing the silo says what it deletes before offering to", async ({ page }) => {
  await page.getByRole("button", { name: "Advanced", exact: true }).click();

  await expect(page.getByText("Takes Personal out of the list on this computer")).toBeVisible();
  await expect(page.getByRole("button", { name: "Remove this silo" })).toBeVisible();
});

test("the app's settings open before any silo is unlocked", async ({ page }) => {
  await page.goto("/?mock=picker");
  await page.getByRole("button", { name: "App settings" }).click();
  await expect(page.getByRole("tab", { name: "General" })).toBeVisible();
  await page.getByRole("tab", { name: /Updates and about/ }).click();
  await expect(page.getByRole("button", { name: "Check for updates" })).toBeVisible();
  await page.getByRole("button", { name: "Back" }).click();
  await expect(page.getByText("Your silos")).toBeVisible();
});
