import { test, expect } from "@playwright/test";

test("unlocking opens the files", async ({ page }) => {
  await page.goto("/?mock=unlock");
  await expect(
    page.getByText("Security key ready. Windows will show a native prompt."),
  ).toBeVisible();

  await page.getByRole("button", { name: "Unlock" }).click();
  await expect(page.getByText("Invoices")).toBeVisible();
});

test("the recovery path is reachable and refuses an empty code", async ({ page }) => {
  await page.goto("/?mock=unlock");
  await page.getByRole("button", { name: "Lost your key? Use your recovery code" }).click();

  await expect(page.getByRole("heading", { name: "Recovery code" })).toBeVisible();
  // Nothing typed, nothing to submit: the button must not invite a click
  // that can only fail.
  await expect(page.getByRole("button", { name: "Unlock" })).toBeDisabled();

  await page.getByRole("button", { name: "Back" }).click();
  await expect(
    page.getByText("Security key ready. Windows will show a native prompt."),
  ).toBeVisible();
});

test("shell uploads queued while locked surface right after unlock", async ({ page }) => {
  await page.goto("/?mock=unlock&dialogs");
  await page.getByRole("button", { name: "Unlock" }).click();

  await expect(page.getByRole("heading", { name: "Add to SilentSilo" })).toBeVisible();
  await expect(
    page.getByText("2 items from Windows Explorer. Choose where they should go."),
  ).toBeVisible();
});
