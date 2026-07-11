import { APP_ROUTES } from "../src/app/routes";
import { blankIconDescriptions, expect, test, waitForRoute } from "./fixtures/api";

const registeredRoutes = APP_ROUTES.map(([id]) => String(id));
const legacyRedirects = {
  feed: "runs",
  swarm: "orchestrator",
  packs: "automations",
  teams: "automations",
} as const;

const routeIdentity: Record<string, string> = {
  dashboard: "Overview",
  chat: "Chat",
  planner: "Planner",
  workflows: "Workflows",
  marketplace: "Marketplace",
  studio: "Studio",
  automations: "Automations",
  webhooks: "Webhooks",
  experiments: "Experiments",
  "enterprise-admin": "Enterprise",
  coding: "Coder",
  agents: "Agents",
  orchestrator: "Task Board",
  files: "Files",
  memory: "Memory",
  runs: "Runs",
  "control-loop": "Control Loop",
  "slack-receipts": "Slack Receipts",
  approvals: "Approvals Inbox",
  settings: "Settings",
  channels: "Channels",
  mcp: "MCP",
  "incident-monitor": "Incident Monitor",
  "packs-detail": "Packs",
  "teams-detail": "Teams",
};

const canonicalRoutes = registeredRoutes.filter((routeId) => !(routeId in legacyRedirects));

async function expectVisibleRouteIdentity(page: Parameters<typeof waitForRoute>[0], routeId: string) {
  await expect(
    page
      .locator("main")
      .getByText(routeIdentity[routeId], { exact: true })
      .filter({ visible: true })
      .first()
  ).toBeVisible();
}

test("route inventory has no duplicate registrations", () => {
  expect(new Set(registeredRoutes).size).toBe(registeredRoutes.length);
  expect(registeredRoutes.length).toBeGreaterThan(20);
  expect(Object.keys(routeIdentity).sort()).toEqual(canonicalRoutes.sort());
});

for (const routeId of canonicalRoutes) {
  test(`${routeId} renders its route identity without browser or icon errors`, async ({ page }) => {
    const browserErrors: string[] = [];
    page.on("pageerror", (error) => browserErrors.push(error.message));

    await page.goto(`/#/${routeId}`);
    await waitForRoute(page, routeId);
    await expectVisibleRouteIdentity(page, routeId);
    expect(await blankIconDescriptions(page), `blank icons on #/${routeId}`).toEqual([]);
    expect(browserErrors, `browser errors on #/${routeId}`).toEqual([]);
  });
}

for (const [legacyRoute, expectedRoute] of Object.entries(legacyRedirects)) {
  test(`${legacyRoute} explicitly redirects to ${expectedRoute}`, async ({ page }) => {
    await page.goto(`/#/${legacyRoute}`);
    await waitForRoute(page, expectedRoute);
    await expectVisibleRouteIdentity(page, expectedRoute);
  });
}

test("computed application typography uses no more than eight font sizes", async ({ page }) => {
  const samplesBySize = new Map<string, string[]>();

  for (const routeId of canonicalRoutes) {
    await page.goto(`/#/${routeId}`);
    await waitForRoute(page, routeId);
    const routeSamples = await page.locator("#app").evaluate((app, currentRoute) => {
      const excluded = "pre, code, .prose, .prose *, .markdown, .markdown *, [class*='markdown'], [class*='markdown'] *";
      const samples: Record<string, string[]> = {};
      for (const element of app.querySelectorAll<HTMLElement>("*")) {
        if (element.matches(excluded) || !element.textContent?.trim()) continue;
        const rect = element.getBoundingClientRect();
        const style = getComputedStyle(element);
        if (!rect.width || !rect.height || style.display === "none" || style.visibility === "hidden") {
          continue;
        }
        const hasDirectText = [...element.childNodes].some(
          (node) => node.nodeType === Node.TEXT_NODE && node.textContent?.trim()
        );
        if (!hasDirectText) continue;
        const descriptor = [
          `#/${currentRoute}`,
          element.tagName.toLowerCase(),
          element.className ? `.${String(element.className).trim().split(/\s+/).slice(0, 3).join(".")}` : "",
          JSON.stringify(element.textContent.trim().replace(/\s+/g, " ").slice(0, 80)),
        ].join(" ");
        (samples[style.fontSize] ||= []).push(descriptor);
      }
      return samples;
    }, routeId);

    for (const [fontSize, samples] of Object.entries(routeSamples)) {
      const aggregate = samplesBySize.get(fontSize) || [];
      aggregate.push(...samples.slice(0, Math.max(0, 4 - aggregate.length)));
      samplesBySize.set(fontSize, aggregate);
    }
  }

  const diagnostics = [...samplesBySize.entries()]
    .sort(([left], [right]) => Number.parseFloat(left) - Number.parseFloat(right))
    .map(([fontSize, samples]) => `${fontSize}:\n  ${samples.join("\n  ")}`)
    .join("\n");
  expect(samplesBySize.size, `Computed font-size union:\n${diagnostics}`).toBeLessThanOrEqual(8);
});

test("unknown routes fall back to the dashboard", async ({ page }) => {
  await page.goto("/#/definitely-not-a-route");
  await waitForRoute(page, "dashboard");
  await expectVisibleRouteIdentity(page, "dashboard");
});
