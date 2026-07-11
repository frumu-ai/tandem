import { APP_ROUTES, ensureRouteId } from "../src/app/routes";
import { blankIconDescriptions, expect, test, waitForRoute } from "./fixtures/api";

const registeredRoutes = APP_ROUTES.map(([id]) => String(id));
const legacyRoutes = ["feed", "swarm"];

test("route inventory has no duplicate registrations", () => {
  expect(new Set(registeredRoutes).size).toBe(registeredRoutes.length);
  expect(registeredRoutes.length).toBeGreaterThan(20);
});

test("every registered and legacy route settles without blank icons", async ({ page }) => {
  const browserErrors: string[] = [];
  page.on("pageerror", (error) => browserErrors.push(error.message));

  for (const routeId of [...registeredRoutes, ...legacyRoutes]) {
    const expectedRoute = ensureRouteId(routeId);
    await test.step(`${routeId} -> ${expectedRoute}`, async () => {
      await page.goto(`/#/${routeId}`);
      await waitForRoute(page, expectedRoute);
      await expect(page.locator("#app")).not.toBeEmpty();
      expect(await blankIconDescriptions(page), `blank icons on #/${routeId}`).toEqual([]);
      expect(browserErrors.splice(0), `browser errors on #/${routeId}`).toEqual([]);
    });
  }
});

test("unknown routes fall back to the dashboard", async ({ page }) => {
  await page.goto("/#/definitely-not-a-route");
  await waitForRoute(page, "dashboard");
});
