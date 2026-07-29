import { expect, test, waitForRoute } from "./fixtures/api";

const CURRENT_ACCOUNT_KEY = "tandem-control-panel-notes:e2e-user";
const PREVIOUS_ACCOUNT_KEY = "tandem-control-panel-notes:previous-user";

test("notes share page state and persist only for the authenticated principal", async ({ page }) => {
  await page.addInitScript(({ previousAccountKey }) => {
    localStorage.setItem(
      previousAccountKey,
      JSON.stringify([
        {
          id: "previous-note",
          title: "Previous account note",
          content: "must remain private",
          createdAt: 1,
          updatedAt: 1,
        },
      ])
    );
  }, { previousAccountKey: PREVIOUS_ACCOUNT_KEY });

  await page.goto("/#/notes");
  await waitForRoute(page, "notes");
  await expect(page.getByText("Previous account note", { exact: true })).toHaveCount(0);

  await page.getByRole("button", { name: "New Note" }).click();
  const titleInput = page.getByPlaceholder("Note title");
  const contentInput = page.getByPlaceholder("Start typing your note here...");
  await expect(titleInput).toHaveValue("New Note");
  await titleInput.fill("Release checklist");
  await contentInput.fill("Verify the release branch is clean.");

  const storedNotes = await page.evaluate(
    ({ currentAccountKey, previousAccountKey }) => ({
      current: localStorage.getItem(currentAccountKey),
      previous: localStorage.getItem(previousAccountKey),
      legacy: localStorage.getItem("tandem-control-panel-notes"),
    }),
    {
      currentAccountKey: CURRENT_ACCOUNT_KEY,
      previousAccountKey: PREVIOUS_ACCOUNT_KEY,
    }
  );
  expect(JSON.parse(storedNotes.current || "[]")).toMatchObject([
    {
      title: "Release checklist",
      content: "Verify the release branch is clean.",
    },
  ]);
  expect(JSON.parse(storedNotes.previous || "[]")).toMatchObject([
    { title: "Previous account note" },
  ]);
  expect(storedNotes.legacy).toBeNull();

  await page.evaluate(() => {
    window.location.hash = "#/dashboard";
  });
  await waitForRoute(page, "dashboard");
  await page.evaluate(() => {
    window.location.hash = "#/notes";
  });
  await waitForRoute(page, "notes");
  await page.getByText("Release checklist", { exact: true }).click();
  await expect(page.getByPlaceholder("Note title")).toHaveValue("Release checklist");
  await expect(page.getByPlaceholder("Start typing your note here...")).toHaveValue(
    "Verify the release branch is clean."
  );
});

test("notes report persistence failures without displaying unsaved state", async ({ page }) => {
  await page.addInitScript(() => {
    const originalSetItem = Storage.prototype.setItem;
    Storage.prototype.setItem = function setItem(key: string, value: string) {
      if (key.startsWith("tandem-control-panel-notes:")) {
        throw new DOMException("Storage is unavailable", "QuotaExceededError");
      }
      return originalSetItem.call(this, key, value);
    };
  });

  await page.goto("/#/notes");
  await waitForRoute(page, "notes");
  await page.getByRole("button", { name: "New Note" }).click();

  await expect(page.getByRole("alert")).toContainText(
    "Notes could not be saved in this browser"
  );
  await expect(page.getByPlaceholder("Note title")).toHaveCount(0);
  await expect(page.getByText("Create your first note to get started", { exact: true })).toBeVisible();
});
