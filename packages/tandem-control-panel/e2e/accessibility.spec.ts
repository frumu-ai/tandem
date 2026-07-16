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
  "settings",
  "planner",
  "studio",
  "automations",
  "orchestrations",
  "goal-operations",
] as const;

const wcagTags = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa"];

test("Porcelain automation builder has readable text contrast", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("tandem.themeId", "porcelain");
  });
  await page.goto("/#/automations");
  await waitForRoute(page, "automations");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "porcelain");
  const results = await new AxeBuilder({ page })
    .include('[data-testid="route-outlet"]')
    .withRules(["color-contrast"])
    .analyze();
  expect(results.violations).toEqual([]);
});

test("application shell and rendered dashboard have no WCAG A/AA violations", async ({ page }) => {
  await page.goto("/#/dashboard");
  await waitForRoute(page, "dashboard");
  const results = await new AxeBuilder({ page }).withTags(wcagTags).analyze();
  expect(results.violations).toEqual([]);
});

for (const routeId of primaryRoutes) {
  test(`${routeId} has no unexpected WCAG A/AA violations`, async ({ page }) => {
    await page.goto(`/#/${routeId}`);
    await waitForRoute(page, routeId);
    const results = await new AxeBuilder({ page })
      .include('[data-testid="route-outlet"]')
      .withTags(wcagTags)
      .analyze();
    expect(results.violations).toEqual([]);
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
      const label = await focused.evaluate(
        (element) =>
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
