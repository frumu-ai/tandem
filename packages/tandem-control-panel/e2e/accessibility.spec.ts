import AxeBuilder from "@axe-core/playwright";
import { expect, test, waitForRoute } from "./fixtures/api";

const primaryRoutes = [
  "dashboard",
  "chat",
  "orchestrator",
  "memory",
  "runs",
  "slack-receipts",
  "approvals",
] as const;

// Existing component/style debt outside this Playwright-owned change. Keep this
// route-specific so every other axe rule remains a required CI failure.
const existingViolationIds: Partial<Record<(typeof primaryRoutes)[number], string[]>> = {
  dashboard: ["color-contrast"],
  chat: ["color-contrast"],
  "slack-receipts": ["select-name"],
};

for (const routeId of primaryRoutes) {
  test(`${routeId} has no unexpected WCAG A/AA violations`, async ({ page }) => {
    await page.goto(`/#/${routeId}`);
    await waitForRoute(page, routeId);
    const results = await new AxeBuilder({ page })
      .include('[data-testid="route-outlet"]')
      .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"])
      .analyze();
    const unexpected = results.violations.filter(
      (violation) => !existingViolationIds[routeId]?.includes(violation.id)
    );
    expect(unexpected).toEqual([]);
  });
}

test("primary navigation and page controls are keyboard reachable", async ({ page }) => {
  await page.goto("/#/dashboard");
  await waitForRoute(page, "dashboard");

  await page.locator("body").press("Tab");
  const focusTrail: string[] = [];
  for (let attempt = 0; attempt < 30 && focusTrail.length < 5; attempt += 1) {
    const focused = page.locator(":focus");
    if (await focused.isVisible().catch(() => false)) {
      const label = await focused.evaluate((element) =>
        element.getAttribute("aria-label") ||
        element.getAttribute("title") ||
        element.textContent?.trim() ||
        element.tagName
      );
      expect(label).toBeTruthy();
      focusTrail.push(String(label));
    }
    await page.keyboard.press("Tab");
  }
  expect(focusTrail.length).toBeGreaterThanOrEqual(5);
  expect(new Set(focusTrail).size).toBeGreaterThanOrEqual(3);
});
