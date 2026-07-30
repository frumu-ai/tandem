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

test("notes create fallback IDs when randomUUID is unavailable", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(globalThis.crypto, "randomUUID", {
      configurable: true,
      value: undefined,
    });
  });

  await page.goto("/#/notes");
  await waitForRoute(page, "notes");
  await page.getByRole("button", { name: "New Note" }).click();

  await expect(page.getByPlaceholder("Note title")).toHaveValue("New Note");
  const storedId = await page.evaluate((storageKey) => {
    const stored = JSON.parse(localStorage.getItem(storageKey) || "[]");
    return String(stored[0]?.id || "");
  }, CURRENT_ACCOUNT_KEY);
  expect(storedId).toMatch(/^note-[0-9a-f]+$/);
});

test("note selection is keyboard accessible and stale edits merge with current storage", async ({
  page,
}) => {
  await page.addInitScript((storageKey) => {
    localStorage.setItem(
      storageKey,
      JSON.stringify([
        {
          id: "shared-note",
          title: "Shared note",
          content: "Original content",
          createdAt: 1,
          updatedAt: 1,
        },
      ])
    );
  }, CURRENT_ACCOUNT_KEY);

  await page.goto("/#/notes");
  await waitForRoute(page, "notes");

  const openSharedNote = page.getByRole("button", { name: "Open note Shared note" });
  await openSharedNote.focus();
  await expect(openSharedNote).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByPlaceholder("Note title")).toHaveValue("Shared note");

  await page.evaluate((storageKey) => {
    localStorage.setItem(
      storageKey,
      JSON.stringify([
        {
          id: "external-note",
          title: "External note",
          content: "Created in another tab",
          createdAt: 2,
          updatedAt: 2,
        },
        {
          id: "shared-note",
          title: "Shared note",
          content: "Changed in another tab",
          createdAt: 1,
          updatedAt: 2,
        },
      ])
    );
  }, CURRENT_ACCOUNT_KEY);

  await expect(page.getByPlaceholder("Start typing your note here...")).toHaveValue(
    "Original content"
  );
  await page.getByPlaceholder("Note title").fill("Locally renamed");
  await expect(page.getByPlaceholder("Start typing your note here...")).toHaveValue(
    "Changed in another tab"
  );

  const mergedNotes = await page.evaluate((storageKey) => {
    return JSON.parse(localStorage.getItem(storageKey) || "[]");
  }, CURRENT_ACCOUNT_KEY);
  expect(mergedNotes).toMatchObject([
    { id: "external-note", title: "External note" },
    {
      id: "shared-note",
      title: "Locally renamed",
      content: "Changed in another tab",
    },
  ]);

  await page.evaluate((storageKey) => {
    const previousValue = localStorage.getItem(storageKey);
    const stored = JSON.parse(previousValue || "[]");
    const nextValue = JSON.stringify([
      {
        id: "synchronized-note",
        title: "Synchronized note",
        content: "Delivered by a storage event",
        createdAt: 3,
        updatedAt: 3,
      },
      ...stored,
    ]);
    localStorage.setItem(storageKey, nextValue);
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: storageKey,
        newValue: nextValue,
        oldValue: previousValue,
        storageArea: localStorage,
        url: window.location.href,
      })
    );
  }, CURRENT_ACCOUNT_KEY);
  await expect(page.getByRole("button", { name: "Open note Synchronized note" })).toBeVisible();
});
