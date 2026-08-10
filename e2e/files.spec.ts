import { test, expect } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/?mock=unlocked");
  await expect(page.getByText("Passport scan.pdf")).toBeVisible();
});

test("the grid and the list show the same silo", async ({ page }) => {
  await page.getByRole("button", { name: "Switch to list view" }).click();

  await expect(page.getByRole("columnheader", { name: "Name" })).toBeVisible();
  // The regression fixed on 3 August 2026: a date carrying its year must
  // stay on one line, in one cell, not fold into two rows' worth of height.
  await expect(page.getByRole("cell", { name: "Nov 15, 2023, 12:00" }).first()).toBeVisible();

  await page.getByRole("button", { name: "Switch to grid view" }).click();
  await expect(page.getByText("896.5 MB · Nov 15, 2023")).toBeVisible();
});

test("the context menu offers the file's whole verb set", async ({ page }) => {
  await page.getByText("Passport scan.pdf").click({ button: "right" });

  for (const verb of ["Open", "Save a copy", "Rename", "Info", "Trash"]) {
    await expect(page.getByRole("menuitem", { name: verb })).toBeVisible();
  }
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menuitem", { name: "Open" })).toHaveCount(0);
});

test("search finds a file by part of its name", async ({ page }) => {
  await page.getByPlaceholder("Search this silo…").fill("archive");
  // The results replace the folder view: what does not match disappears,
  // and the hit is the one thing left on screen.
  await expect(page.getByText("Passport scan.pdf")).toHaveCount(0);
  await expect(page.getByText("Archive 2019.zip")).toBeVisible();
});

test("sections render what the sidebar promises", async ({ page }) => {
  await page.getByRole("button", { name: "Trash" }).click();
  await expect(page.getByText("4 items, restorable until deleted for good")).toBeVisible();
  await expect(page.getByText("Old contract.pdf")).toBeVisible();
  await expect(page.getByRole("button", { name: "Empty trash" })).toBeVisible();

  await page.getByRole("button", { name: "Health" }).click();
  await expect(page.getByText("3 things to look at across 7 entries")).toBeVisible();
  await expect(page.getByText("1 password is used more than once")).toBeVisible();
});
