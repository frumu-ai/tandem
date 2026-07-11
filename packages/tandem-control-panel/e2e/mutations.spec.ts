import { blankIconDescriptions, expect, test, waitForRoute } from "./fixtures/api";

test("save icon survives the complete mutation loading lifecycle", async ({ page, apiFixture }) => {
  await page.goto("/#/settings");
  await waitForRoute(page, "settings");

  const save = page.getByRole("button", { name: "Save config" });
  await expect(save).toBeEnabled();
  await expect(save.locator("svg.lucide-save")).toHaveCount(1);
  await expect(save.locator("svg.lucide-save > *")).not.toHaveCount(0);

  const held = apiFixture.holdNext("/api/control-panel/config", "PATCH");
  await save.click();
  await held.waitUntilRequested();
  await expect(save).toBeDisabled();
  await expect(save.locator("svg.lucide-save > *")).not.toHaveCount(0);
  expect(await blankIconDescriptions(page)).toEqual([]);

  held.release();
  await expect(save).toBeEnabled();
  await expect(save.locator("svg.lucide-save > *")).not.toHaveCount(0);
  expect(await blankIconDescriptions(page)).toEqual([]);
});
