import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/?mock=unlocked");
  await page.getByRole("button", { name: "Credentials" }).click();
  await expect(page.getByText("All items")).toBeVisible();
});

test("selecting an entry shows its details", async ({ page }) => {
  await expect(page.getByText("Select an item to see its details.")).toBeVisible();

  await page.getByText("Bank", { exact: true }).first().click();
  // The notes line only exists in the detail pane, so it is the one proof
  // the pane rendered this entry rather than any entry.
  await expect(page.getByText("Branch phone: 021 000 000")).toBeVisible();
});

test("the type filter narrows the list", async ({ page }) => {
  await page.getByText("Cards", { exact: true }).click();

  await expect(page.getByText("Personal Visa")).toBeVisible();
  await expect(page.getByText("GitHub")).toHaveCount(0);
});
