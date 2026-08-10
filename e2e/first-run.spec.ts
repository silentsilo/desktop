import { test, expect } from "@playwright/test";

// The first five minutes of the product: no silo, create one, choose what
// unlocks it, land in the files. Nothing else in the app matters if this
// path breaks, and no unit test can see it.

test("creating a silo leads into enrolment and out to the files", async ({ page }) => {
  await page.goto("/?mock=empty");
  await expect(page.getByRole("heading", { name: "New silo" })).toBeVisible();

  // The form arrives pre-filled with a name and a location, so the happy
  // path is one click.
  await expect(page.getByRole("textbox").first()).toHaveValue("Personal");
  await page.getByRole("button", { name: "Create silo" }).click();

  await expect(page.getByRole("heading", { name: "Set up unlocking" })).toBeVisible();
  await page.getByRole("button", { name: "Use a security key" }).click();

  await expect(
    page.getByText("Security key enrolled. It opens this silo from now on."),
  ).toBeVisible();
  await expect(page.getByText("Invoices")).toBeVisible();
});

test("the organisation choice reaches the backend, and is off unless asked for", async ({
  page,
}) => {
  // The bug this exists for: the tick was read correctly and then dropped by
  // the handler between the view and the command, so a silo somebody set up as
  // organisation-administered was enrolled as an ordinary one. Nothing on
  // screen said so, and the enrolment cannot be redone.
  await page.goto("/?mock=empty");
  await page.getByRole("button", { name: "Create silo" }).click();
  await expect(page.getByRole("heading", { name: "Set up unlocking" })).toBeVisible();

  const sent: unknown[] = [];
  await page.exposeFunction("__recordEnrol", (args: unknown) => void sent.push(args));
  await page.evaluate(() => {
    const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: unknown } })
      .__TAURI_INTERNALS__;
    const original = internals.invoke as (...a: unknown[]) => unknown;
    internals.invoke = function (this: unknown, ...args: unknown[]) {
      if (args[0] === "fido_enroll_primary") {
        (window as unknown as { __recordEnrol: (a: unknown) => void }).__recordEnrol(args[1]);
      }
      return original.apply(this, args);
    };
  });

  const organisation = page.getByRole("checkbox", {
    name: /administered by an organisation/i,
  });
  await expect(organisation).not.toBeChecked();
  await organisation.check();
  await page.getByRole("button", { name: "Use a security key" }).click();

  await expect(page.getByText("Security key enrolled. It opens this silo from now on.")).toBeVisible();
  expect(sent).toEqual([{ authenticator: "security-key", organisation: true }]);
});

test("backing out of enrolment discards the silo, after asking", async ({ page }) => {
  await page.goto("/?mock=empty");
  await page.getByRole("button", { name: "Create silo" }).click();
  await expect(page.getByRole("heading", { name: "Set up unlocking" })).toBeVisible();

  await page.getByRole("button", { name: "discard it" }).click();
  await expect(page.getByText("Discard this silo?")).toBeVisible();
  await page.getByRole("button", { name: "Discard silo" }).click();

  // Back where first-run starts: nothing exists, so the create form is the
  // screen again.
  await expect(page.getByRole("heading", { name: "New silo" })).toBeVisible();
});

test("a machine without Windows Hello only offers the security key", async ({ page }) => {
  await page.goto("/?mock=empty&nohello");
  await page.getByRole("button", { name: "Create silo" }).click();

  await expect(page.getByRole("button", { name: "Use a security key" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Use Windows Hello" })).toHaveCount(0);
});

test("the two ways in are offered once, where they are the only way", async ({ page }) => {
  // First run has no list behind the form, so someone whose silo already
  // exists needs both of these here or they are stuck creating one.
  await page.goto("/?mock=empty");
  await expect(page.getByText("Already have one?")).toBeVisible();

  // With a list, the form is one click from a screen that offers both, and
  // repeating them reads as a second, different pair of choices.
  await page.goto("/?mock=picker");
  await expect(
    page.getByRole("button", { name: "Add a folder from this computer" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "New silo" }).click();
  await expect(page.getByRole("heading", { name: "New silo" })).toBeVisible();
  await expect(page.getByText("Already have one?")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Back" })).toBeVisible();
});

test("one name per way in, on both screens", async ({ page }) => {
  // The same two actions used to carry four names between them.
  await page.goto("/?mock=empty");
  await expect(page.getByText("Already have one?")).toBeVisible();
  const onFirstRun = await page.locator(".auth-alternatives button").allInnerTexts();

  await page.goto("/?mock=picker");
  await expect(page.getByRole("heading", { name: "Your silos" })).toBeVisible();
  const onPicker = await page.locator(".silo-actions button").allInnerTexts();

  for (const label of onFirstRun) {
    expect(onPicker).toContain(label);
  }
});
