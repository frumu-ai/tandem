import type { Page } from "@playwright/test";
import { expect, test, waitForRoute } from "./fixtures/api";

const CURRENT_ACCOUNT_KEY = "tandem-control-panel-notes:e2e-user";
const PREVIOUS_ACCOUNT_KEY = "tandem-control-panel-notes:previous-user";

async function answerConfirmation(
  page: Page,
  action: () => Promise<unknown>,
  accept: boolean,
  expectedMessage: string
) {
  const dialogPromise = page.waitForEvent("dialog");
  const actionPromise = action();
  const dialog = await dialogPromise;
  try {
    expect(dialog.type()).toBe("confirm");
    expect(dialog.message()).toContain(expectedMessage);
  } finally {
    if (accept) await dialog.accept();
    else await dialog.dismiss();
    await actionPromise;
  }
}

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

  await expect
    .poll(async () => {
      const stored = await page.evaluate((storageKey) => {
        return JSON.parse(localStorage.getItem(storageKey) || "[]")[0]?.content;
      }, CURRENT_ACCOUNT_KEY);
      return stored;
    })
    .toBe("Verify the release branch is clean.");

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

test("note edits update immediately and persist in one debounced write", async ({ page }) => {
  await page.clock.install();
  await page.addInitScript((storageKey) => {
    localStorage.setItem(
      storageKey,
      JSON.stringify([
        {
          id: "debounced-note",
          title: "Original",
          content: "Original content",
          createdAt: 1,
          updatedAt: 1,
        },
      ])
    );
    const trackedWindow = window as typeof window & { __notesWriteCount: number };
    trackedWindow.__notesWriteCount = 0;
    const originalSetItem = Storage.prototype.setItem;
    Storage.prototype.setItem = function setItem(key: string, value: string) {
      if (key === storageKey) trackedWindow.__notesWriteCount += 1;
      return originalSetItem.call(this, key, value);
    };
  }, CURRENT_ACCOUNT_KEY);

  await page.goto("/#/notes");
  await waitForRoute(page, "notes");
  await page.getByRole("button", { name: "Open note Original" }).click();
  const currentTime = await page.evaluate(() => Date.now());
  await page.clock.pauseAt(currentTime + 60_000);

  const titleInput = page.getByPlaceholder("Note title");
  await titleInput.press("End");
  await titleInput.pressSequentially(" 2");
  expect(await titleInput.inputValue()).toBe("Original 2");
  await expect(page.getByRole("button", { name: "Open note Original 2" })).toBeVisible();
  await page.clock.runFor(399);
  expect(
    await page.evaluate(
      () => (window as typeof window & { __notesWriteCount: number }).__notesWriteCount
    )
  ).toBe(0);

  await page.clock.runFor(1);
  expect(
    await page.evaluate(
      () => (window as typeof window & { __notesWriteCount: number }).__notesWriteCount
    )
  ).toBe(1);
  const storedTitle = await page.evaluate((storageKey) => {
    return JSON.parse(localStorage.getItem(storageKey) || "[]")[0]?.title;
  }, CURRENT_ACCOUNT_KEY);
  expect(storedTitle).toBe("Original 2");

  await titleInput.press("End");
  await titleInput.pressSequentially(" blurred");
  expect(await titleInput.inputValue()).toBe("Original 2 blurred");
  expect(
    await page.evaluate(
      () => (window as typeof window & { __notesWriteCount: number }).__notesWriteCount
    )
  ).toBe(1);
  await page.getByRole("button", { name: "New Note" }).focus();
  expect(
    await page.evaluate(
      () => (window as typeof window & { __notesWriteCount: number }).__notesWriteCount
    )
  ).toBe(2);
  const blurFlushedTitle = await page.evaluate((storageKey) => {
    return JSON.parse(localStorage.getItem(storageKey) || "[]")[0]?.title;
  }, CURRENT_ACCOUNT_KEY);
  expect(blurFlushedTitle).toBe("Original 2 blurred");
  await page.clock.runFor(400);
  expect(
    await page.evaluate(
      () => (window as typeof window & { __notesWriteCount: number }).__notesWriteCount
    )
  ).toBe(2);
});

test("debounced note edits roll back when persistence fails", async ({ page }) => {
  await page.clock.install();
  await page.addInitScript((storageKey) => {
    localStorage.setItem(
      storageKey,
      JSON.stringify([
        {
          id: "rejected-note",
          title: "Stored title",
          content: "Stored content",
          createdAt: 1,
          updatedAt: 1,
        },
      ])
    );
    const trackedWindow = window as typeof window & {
      __failNotesWrites: boolean;
      __notesWriteCount: number;
    };
    trackedWindow.__failNotesWrites = false;
    trackedWindow.__notesWriteCount = 0;
    const originalSetItem = Storage.prototype.setItem;
    Storage.prototype.setItem = function setItem(key: string, value: string) {
      if (key === storageKey) {
        trackedWindow.__notesWriteCount += 1;
        if (trackedWindow.__failNotesWrites) {
          throw new DOMException("Storage is unavailable", "QuotaExceededError");
        }
      }
      return originalSetItem.call(this, key, value);
    };
  }, CURRENT_ACCOUNT_KEY);

  await page.goto("/#/notes");
  await waitForRoute(page, "notes");
  await page.getByRole("button", { name: "Open note Stored title" }).click();
  await page.evaluate(() => {
    (window as typeof window & { __failNotesWrites: boolean }).__failNotesWrites = true;
  });
  const currentTime = await page.evaluate(() => Date.now());
  await page.clock.pauseAt(currentTime + 60_000);

  const titleInput = page.getByPlaceholder("Note title");
  await titleInput.fill("Rejected rename");
  await page.clock.runFor(399);
  expect(await titleInput.inputValue()).toBe("Rejected rename");
  expect(
    await page.evaluate(
      () => (window as typeof window & { __notesWriteCount: number }).__notesWriteCount
    )
  ).toBe(0);

  await page.clock.runFor(1);
  await expect(page.getByRole("alert")).toContainText(
    "Notes could not be saved in this browser"
  );
  await expect(titleInput).toHaveValue("Stored title");
  const rejectedState = await page.evaluate((storageKey) => ({
    notes: JSON.parse(localStorage.getItem(storageKey) || "[]"),
    writes: (window as typeof window & { __notesWriteCount: number }).__notesWriteCount,
  }), CURRENT_ACCOUNT_KEY);
  expect(rejectedState.notes).toMatchObject([{ title: "Stored title" }]);
  expect(rejectedState.writes).toBe(1);
  await page.clock.runFor(400);
  expect(
    await page.evaluate(
      () => (window as typeof window & { __notesWriteCount: number }).__notesWriteCount
    )
  ).toBe(1);
});

test("invalid stored notes recover through storage events or an explicit reset", async ({
  page,
}) => {
  await page.addInitScript((storageKey) => {
    localStorage.setItem(storageKey, "{");
  }, CURRENT_ACCOUNT_KEY);

  await page.goto("/#/notes");
  await waitForRoute(page, "notes");
  await expect(page.getByRole("alert")).toContainText(
    "Notes could not be loaded for this account"
  );
  await expect(page.getByRole("button", { name: "Reset local notes" })).toBeVisible();

  await page.evaluate((storageKey) => {
    const oldValue = localStorage.getItem(storageKey);
    const newValue = JSON.stringify([
      {
        id: "recovered-note",
        title: "Recovered note",
        content: "Recovered from another tab",
        createdAt: 1,
        updatedAt: 1,
      },
    ]);
    localStorage.setItem(storageKey, newValue);
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: storageKey,
        oldValue,
        newValue,
        storageArea: localStorage,
        url: window.location.href,
      })
    );
  }, CURRENT_ACCOUNT_KEY);
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Open note Recovered note" })).toBeVisible();

  await page.evaluate((storageKey) => {
    const oldValue = localStorage.getItem(storageKey);
    const newValue = JSON.stringify([{ id: "invalid-note" }]);
    localStorage.setItem(storageKey, newValue);
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: storageKey,
        oldValue,
        newValue,
        storageArea: localStorage,
        url: window.location.href,
      })
    );
  }, CURRENT_ACCOUNT_KEY);
  await expect(page.getByRole("alert")).toContainText(
    "Notes could not be loaded for this account"
  );
  await expect(page.getByRole("button", { name: "Open note Recovered note" })).toBeVisible();

  const resetButton = page.getByRole("button", { name: "Reset local notes" });
  await answerConfirmation(
    page,
    () => resetButton.click(),
    false,
    "permanently removes the corrupted data"
  );
  expect(await page.evaluate((key) => localStorage.getItem(key), CURRENT_ACCOUNT_KEY)).toBe(
    JSON.stringify([{ id: "invalid-note" }])
  );

  await answerConfirmation(page, () => resetButton.click(), true, "Reset all local notes");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Open note Recovered note" })).toHaveCount(0);
  expect(await page.evaluate((key) => localStorage.getItem(key), CURRENT_ACCOUNT_KEY)).toBeNull();

  await page.getByRole("button", { name: "New Note" }).click();
  await expect(page.getByPlaceholder("Note title")).toHaveValue("New Note");
});

test("note deletion requires confirmation", async ({ page }) => {
  await page.addInitScript((storageKey) => {
    localStorage.setItem(
      storageKey,
      JSON.stringify([
        {
          id: "protected-note",
          title: "Protected note",
          content: "Do not delete accidentally",
          createdAt: 1,
          updatedAt: 1,
        },
      ])
    );
  }, CURRENT_ACCOUNT_KEY);

  await page.goto("/#/notes");
  await waitForRoute(page, "notes");
  const deleteButton = page.getByRole("button", { name: "Delete note Protected note" });
  await deleteButton.focus();
  await answerConfirmation(
    page,
    () => page.keyboard.press("Enter"),
    false,
    'Delete "Protected note"?'
  );
  await expect(page.getByRole("button", { name: "Open note Protected note" })).toBeVisible();
  const retainedNotes = await page.evaluate((storageKey) => {
    return JSON.parse(localStorage.getItem(storageKey) || "[]");
  }, CURRENT_ACCOUNT_KEY);
  expect(retainedNotes).toMatchObject([{ id: "protected-note" }]);

  await answerConfirmation(
    page,
    () => deleteButton.click(),
    true,
    'Delete "Protected note"?'
  );
  await expect(page.getByRole("button", { name: "Open note Protected note" })).toHaveCount(0);
  const deletedNotes = await page.evaluate((storageKey) => {
    return JSON.parse(localStorage.getItem(storageKey) || "[]");
  }, CURRENT_ACCOUNT_KEY);
  expect(deletedNotes).toEqual([]);
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
          updatedAt: 9_000_000_000_000_000,
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
      updatedAt: 9_000_000_000_000_000,
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
