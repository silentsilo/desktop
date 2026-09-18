import { test, expect } from "@playwright/test";

// The browser extension's fill is confirmed in the app. The question must
// name the site and the login, and say so in words when the login was saved
// for another site.

test("a fill for the site the login was saved for asks plainly", async ({ page }) => {
  await page.goto("/?mock=unlocked&fill");
  const dialog = page.getByRole("alertdialog", { name: "Fill a login in your browser?" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByText("github.com", { exact: true })).toBeVisible();
  await expect(dialog.locator("dd").nth(1)).toContainText("GitHub");
  await expect(dialog.getByRole("alert")).toHaveCount(0);
  // It says what the app knows, not who asked.
  await expect(dialog).toContainText(
    "A fill request from your browser for github.com. If you did not just click SilentSilo in the browser, choose Cancel.",
  );

  await dialog.getByRole("button", { name: "Fill", exact: true }).click();
  await expect(dialog).toBeHidden();
});

test("a fill somewhere else says where the login belongs", async ({ page }) => {
  await page.goto("/?mock=unlocked&fill=mismatch");
  const dialog = page.getByRole("alertdialog", { name: "Fill a login in your browser?" });
  await expect(dialog.getByRole("alert")).toContainText(
    "This login was saved for bank.example, not for bank-login.example.",
  );
  await expect(dialog.getByRole("button", { name: "Fill anyway" })).toBeVisible();

  await dialog.getByRole("button", { name: "Cancel" }).click();
  await expect(dialog).toBeHidden();
});

test("the setting says the extension cannot see files, and starts off", async ({ page }) => {
  await page.goto("/?mock=unlocked");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("button", { name: "Browser extension" }).click();
  await expect(
    page.getByText("Lets the browser extension fill passwords. It cannot see your files."),
  ).toBeVisible();
  const toggle = page.getByRole("checkbox", { name: /Allow the SilentSilo browser extension/ });
  await expect(toggle).not.toBeChecked();
  await toggle.check();
  await expect(toggle).toBeChecked();
});

test("a build without the host says so and offers no toggle", async ({ page }) => {
  await page.goto("/?mock=unlocked&nohost");
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("button", { name: "Browser extension" }).click();
  await expect(page.getByText("The browser extension is not part of this build.")).toBeVisible();
  await expect(
    page.getByRole("checkbox", { name: /Allow the SilentSilo browser extension/ }),
  ).toHaveCount(0);
});
