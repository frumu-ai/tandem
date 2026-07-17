import AxeBuilder from "@axe-core/playwright";
import { expect, test, waitForRoute, type ApiFixture } from "./fixtures/api";

const runId = "automation-v2-run-responsive";

const canonicalRun = {
  run: {
    run_id: runId,
    kind: "automation_v2",
    automation_id: "automation-responsive",
    status: "awaiting_approval",
    phase: "paused_attention_required",
    trigger_type: "webhook",
    updated_at_ms: 1_788_700_000_000,
    workspace_root: "/srv/tandem/workspaces/a-very-long-responsive-workspace-path",
    automation_snapshot: { name: "Responsive workflow run" },
    scope: {
      tenant_context: {
        org_id: "responsive-org",
        workspace_id: "responsive-workspace",
        deployment_id: "production-eu-central",
      },
    },
  },
  current_wait: {
    wait_kind: "approval",
    status: "waiting",
    reason: "Waiting for a human reviewer to approve the responsive workflow",
  },
  enterprise_scope: {
    owning_org_unit_id: "customer-success",
    owning_org_unit: { display_name: "Customer Success Operations" },
    resource_kind: "repository",
    resource_id: "responsive-repository-with-a-long-identifier",
    policy_version_id: "policy-responsive-v1",
    data_classes: ["customer_data", "support_transcript", "internal_notes"],
    visible_knowledge_sources: [
      { binding_id: "source-1", source_root_label: "Responsive knowledge source" },
    ],
  },
};

function mockCanonicalRun(apiFixture: ApiFixture) {
  apiFixture.mockResponse(/\/api\/engine\/stateful-runtime\/runs\?limit=120$/, {
    runs: [canonicalRun],
  });
  apiFixture.mockResponse(
    new RegExp(`/api/engine/stateful-runtime/runs/${runId}/observability\\?`),
    {
      run: canonicalRun.run,
      events: [],
      snapshots: [],
      reliability: {},
      audit_events: [],
    }
  );
  apiFixture.mockResponse(
    new RegExp(`/api/engine/automations/v2/runs/${runId}$`),
    { run: canonicalRun.run }
  );
}

test("runs stay responsive and open details only on demand", async ({ page, apiFixture }) => {
  test.setTimeout(60_000);
  await page.addInitScript(() => {
    localStorage.setItem("tandem.themeId", "charcoal_fire");
  });
  mockCanonicalRun(apiFixture);

  await page.goto("/#/runs");
  await waitForRoute(page, "runs");

  await expect(page.locator("html")).toHaveAttribute("data-theme", "charcoal_fire");
  const backgroundToken = await page.locator("html").evaluate((element) =>
    getComputedStyle(element).getPropertyValue("--color-background").trim()
  );
  await expect(page.getByText("Responsive workflow run", { exact: true })).toBeVisible();
  await expect(page.getByText("production-eu-central", { exact: true })).toBeVisible();
  await expect(page.getByText("support_transcript", { exact: true })).toBeVisible();
  await expect(page.getByRole("dialog", { name: "Run Detail" })).toHaveCount(0);
  expect(apiFixture.requests.some((request) => request.includes("/observability"))).toBe(false);

  const runList = page.getByTestId("run-list");
  await expect(runList).toBeVisible();
  expect(
    await runList.evaluate((element) => element.scrollWidth <= element.clientWidth + 1)
  ).toBe(true);

  await page.getByRole("button", { name: "Inspect run detail for Responsive workflow run" }).click();
  await expect(page).toHaveURL(new RegExp(`#/runs\\?run=${runId}$`));
  const runDetail = page.getByRole("dialog", { name: "Run Detail" });
  await expect(runDetail).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "charcoal_fire");
  expect(
    await page.locator("html").evaluate((element) =>
      getComputedStyle(element).getPropertyValue("--color-background").trim()
    )
  ).toBe(backgroundToken);
  const overlayChannels = await page.locator(".tcp-confirm-overlay").evaluate((element) => {
    const color = getComputedStyle(element).backgroundColor;
    const values = color.match(/[\d.]+/g)?.map(Number) || [];
    const multiplier = color.startsWith("color(") ? 255 : 1;
    return values.slice(0, 3).map((value) => value * multiplier);
  });
  expect(overlayChannels).toHaveLength(3);
  expect(Math.max(...overlayChannels) - Math.min(...overlayChannels)).toBeLessThan(1.5);
  const contrastResults = await new AxeBuilder({ page })
    .include('[role="dialog"]')
    .withRules(["color-contrast"])
    .analyze();
  expect(contrastResults.violations).toEqual([]);
  await expect(page.getByRole("button", { name: "Open debugger" })).toBeVisible();
  const dialogBox = await runDetail.boundingBox();
  const viewport = page.viewportSize();
  expect(dialogBox?.width || 0).toBeGreaterThan((viewport?.width || 0) * 0.9);
  expect(dialogBox?.height || 0).toBeGreaterThan((viewport?.height || 0) * 0.85);

  await page.getByRole("button", { name: "Open debugger" }).click();
  await waitForRoute(page, "automations");
  await expect(page).toHaveURL(new RegExp(`#/automations\\?run=${runId}$`));
  await expect(page.getByRole("heading", { name: "Run Debugger" })).toBeVisible();

  await page.reload({ waitUntil: "commit" });
  await waitForRoute(page, "automations");
  await expect(page).toHaveURL(new RegExp(`#/automations\\?run=${runId}$`));
  await expect(page.getByRole("heading", { name: "Run Debugger" })).toBeVisible();

  await page.getByRole("button", { name: "Close" }).first().click();
  await expect(page).toHaveURL(/#\/automations$/);
});

test("Porcelain Run Detail has readable text contrast", async ({ page, apiFixture }) => {
  await page.addInitScript(() => {
    localStorage.setItem("tandem.themeId", "porcelain");
  });
  mockCanonicalRun(apiFixture);

  await page.goto("/#/runs");
  await waitForRoute(page, "runs");
  await page.getByRole("button", { name: "Inspect run detail for Responsive workflow run" }).click();

  const runDetail = page.getByRole("dialog", { name: "Run Detail" });
  await expect(runDetail).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "porcelain");
  const results = await new AxeBuilder({ page })
    .include('[role="dialog"]')
    .withRules(["color-contrast"])
    .analyze();
  expect(results.violations).toEqual([]);
});

test("Porcelain loading state uses the dark Tandem blue mark", async ({ page, apiFixture }) => {
  await page.addInitScript(() => {
    localStorage.setItem("tandem.themeId", "porcelain");
  });
  const heldRuns = apiFixture.holdNext(
    /\/api\/engine\/stateful-runtime\/runs$/,
    "GET"
  );

  await page.goto("/#/runs");
  await waitForRoute(page, "runs");
  await expect(page.getByText("Loading runs", { exact: true })).toBeVisible();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "porcelain");
  const baseFill = await page
    .locator(".tcp-tandem-logo-compact .tcp-logo-base")
    .first()
    .evaluate((element) => getComputedStyle(element).fill);
  expect(baseFill).toBe("rgb(30, 58, 138)");

  heldRuns.release();
});
