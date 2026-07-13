import { expect, test, waitForRoute, type ApiFixture } from "./fixtures/api";

const draft = {
  schema_version: 1,
  orchestration_id: "orch-plan-execute",
  name: "Plan and execute",
  description: "A bounded planning loop",
  status: "draft",
  version: 0,
  root_node_id: "plan",
  nodes: [
    {
      node_id: "plan",
      name: "Plan",
      position: { x: 72, y: 120 },
      kind: "workflow",
      automation_id: "automation-plan",
      allowed_transition_keys: ["complete"],
    },
    {
      node_id: "done",
      name: "Complete",
      position: { x: 380, y: 120 },
      kind: "terminal",
      outcome: "complete",
    },
  ],
  edges: [
    {
      edge_id: "edge-complete",
      from_node_id: "plan",
      to_node_id: "done",
      transition_key: "complete",
    },
  ],
  goal_policy: { max_hops: 20, on_limit: "pause_for_review" },
  tenant_context: { org_id: "org-e2e", workspace_id: "workspace-e2e", source: "explicit" },
  created_at_ms: 1_700_000_000_000,
  updated_at_ms: 1_700_000_000_100,
};

function mockOrchestration(apiFixture: ApiFixture) {
  apiFixture.mockResponse("/api/engine/orchestrations", {
    orchestrations: [
      {
        orchestration_id: draft.orchestration_id,
        name: draft.name,
        draft: { status: "draft", updated_at_ms: draft.updated_at_ms },
        latest_published_version: 2,
        published_versions: [{ version: 2, published_at_ms: 1_700_000_000_000 }],
      },
    ],
    count: 1,
  });
  apiFixture.mockResponse("/api/engine/automations/v2", {
    automations: [{ automation_id: "automation-plan", name: "Plan" }],
    count: 1,
  });
  apiFixture.mockResponse("/api/engine/goals", { goals: [], count: 0 });
  apiFixture.mockResponse(`/api/engine/orchestrations/${draft.orchestration_id}`, {
    orchestration_id: draft.orchestration_id,
    draft,
    latest_published: {
      ...draft,
      status: "published",
      version: 2,
      published_at_ms: 1_700_000_000_000,
    },
  });
  apiFixture.mockResponse(`/api/engine/orchestrations/${draft.orchestration_id}/versions`, {
    orchestration_id: draft.orchestration_id,
    versions: [{ version: 2, name: draft.name, published_at_ms: 1_700_000_000_000 }],
    count: 1,
  });
  apiFixture.mockResponse(
    `/api/engine/orchestrations/${draft.orchestration_id}/validate`,
    {
      orchestration_id: draft.orchestration_id,
      version: 0,
      report: { valid: true, issues: [] },
      stale_references: [],
    },
    "POST"
  );
  apiFixture.mockResponse(`/api/engine/orchestrations/${draft.orchestration_id}/stale-references`, {
    orchestration_id: draft.orchestration_id,
    references: [],
    stale_count: 0,
  });
}

test("orchestration library opens a canonical draft in the visual and outline editors", async ({
  page,
  apiFixture,
}) => {
  mockOrchestration(apiFixture);
  let savedBody: any = null;
  let currentDraft: any = draft;
  await page.route(`**/api/engine/orchestrations/${draft.orchestration_id}`, async (route) => {
    if (route.request().method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          orchestration_id: draft.orchestration_id,
          draft: currentDraft,
          latest_published: {
            ...draft,
            status: "published",
            version: 2,
            published_at_ms: 1_700_000_000_000,
          },
        }),
      });
      return;
    }
    if (route.request().method() !== "PUT") return route.fallback();
    savedBody = route.request().postDataJSON();
    currentDraft = {
      ...currentDraft,
      ...savedBody,
      updated_at_ms: currentDraft.updated_at_ms + 1,
    };
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        orchestration: currentDraft,
        orchestration_id: draft.orchestration_id,
        version: 0,
        status: "draft",
        updated_at_ms: currentDraft.updated_at_ms,
      }),
    });
  });
  await page.goto("/#/orchestrations");
  await waitForRoute(page, "orchestrations");
  await expect(
    page.getByTestId("route-outlet").getByRole("heading", { name: "Orchestrations" })
  ).toBeVisible();
  await page.getByRole("button", { name: /Plan and execute/ }).click();

  await expect(page.getByLabel("Orchestration graph editor")).toBeVisible();
  await expect(page.getByText("Version 0", { exact: true })).toBeVisible();
  await page.getByLabel("Orchestration name").fill("Renamed in Studio");
  await page.getByRole("button", { name: "Undo" }).click();
  await expect(page.getByLabel("Orchestration name")).toHaveValue(draft.name);
  await expect(
    page.getByText("Plan", { exact: true }).filter({ visible: true }).first()
  ).toBeVisible();
  const canvas = page.locator(".orch-canvas");
  const box = await canvas.boundingBox();
  expect(box?.width).toBeGreaterThan(300);
  expect(box?.height).toBeGreaterThan(400);
  const renderedPixels = await canvas.evaluate((element) => {
    const style = getComputedStyle(element);
    return (
      element.querySelectorAll(".react-flow__node, .react-flow__edge").length > 0 &&
      style.display !== "none"
    );
  });
  expect(renderedPixels).toBe(true);
  const versionsRequestsBeforeEdits = apiFixture.requests.filter(
    (request) => request === `GET /api/engine/orchestrations/${draft.orchestration_id}/versions`
  ).length;

  await page.getByLabel("Search nodes").fill("timer");
  await expect(page.getByRole("group", { name: "Timer node" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Approval node" })).toHaveCount(0);
  await page.getByLabel("Search nodes").fill("");
  const timerItem = page.getByRole("group", { name: "Timer node" });
  await timerItem.click();
  await expect(page.getByText("timer wait", { exact: true })).toHaveCount(0);

  await page.getByRole("button", { name: "Add Approval node" }).click();
  await expect(
    page.getByText("approval wait", { exact: true }).filter({ visible: true }).first()
  ).toBeVisible();
  await page.getByRole("button", { name: "Remove selected node: approval wait" }).click();
  await expect(page.getByText("approval wait", { exact: true })).toHaveCount(0);

  await page.getByRole("button", { name: "Add Timer node" }).click();
  await expect(
    page.getByText("timer wait", { exact: true }).filter({ visible: true }).first()
  ).toBeVisible();
  await expect(timerItem.getByLabel("1 on canvas")).toBeVisible();
  await page.getByRole("button", { name: "Add Webhook node" }).click();
  await expect(page.getByLabel("Correlation source")).toBeVisible();

  await page.getByRole("tab", { name: "Outline" }).click();
  await expect(page.getByRole("tabpanel")).toBeVisible();
  await page.getByRole("button", { name: /Plan.*root/ }).focus();
  await expect(page.getByRole("button", { name: /Plan.*root/ })).toBeFocused();
  await page.getByLabel("Transition source").selectOption("plan");
  await page.getByLabel("Transition target").selectOption({ label: "timer wait" });
  await page.getByLabel("Transition key").fill("wait");
  await page.getByRole("button", { name: "Add transition" }).click();
  await expect(
    page.getByText("wait", { exact: true }).filter({ visible: true }).first()
  ).toBeVisible();
  await expect
    .poll(() => savedBody?.edges?.some((edge: any) => edge.transition_key === "wait"))
    .toBe(true);
  await page.waitForTimeout(250);
  expect(
    apiFixture.requests.filter(
      (request) => request === `GET /api/engine/orchestrations/${draft.orchestration_id}/versions`
    )
  ).toHaveLength(versionsRequestsBeforeEdits);
  expect(savedBody.expected_updated_at_ms).toBeGreaterThanOrEqual(draft.updated_at_ms);
  expect(
    savedBody.nodes.some((node: any) => node.kind === "wait" && node.wait.kind === "timer")
  ).toBe(true);
  await expect(page.getByText("webhook_trigger_invalid", { exact: true })).toBeVisible();
});

test("edits made while autosave is in flight remain in the draft", async ({ page, apiFixture }) => {
  mockOrchestration(apiFixture);
  let releaseFirstSave!: () => void;
  let markFirstSaveRequested!: () => void;
  const firstSaveReleased = new Promise<void>((resolve) => {
    releaseFirstSave = resolve;
  });
  const firstSaveRequested = new Promise<void>((resolve) => {
    markFirstSaveRequested = resolve;
  });
  const savedBodies: any[] = [];
  let currentDraft: any = draft;

  await page.route(`**/api/engine/orchestrations/${draft.orchestration_id}`, async (route) => {
    if (route.request().method() === "GET") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          orchestration_id: draft.orchestration_id,
          draft: currentDraft,
          latest_published: null,
        }),
      });
      return;
    }
    if (route.request().method() !== "PUT") return route.fallback();
    const body = route.request().postDataJSON();
    savedBodies.push(body);
    if (savedBodies.length === 1) {
      markFirstSaveRequested();
      await firstSaveReleased;
    }
    currentDraft = {
      ...currentDraft,
      ...body,
      updated_at_ms: currentDraft.updated_at_ms + 1,
    };
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        orchestration: currentDraft,
        orchestration_id: draft.orchestration_id,
        version: 0,
        status: "draft",
        updated_at_ms: currentDraft.updated_at_ms,
      }),
    });
  });

  await page.goto("/#/orchestrations");
  await waitForRoute(page, "orchestrations");
  await page.getByRole("button", { name: /Plan and execute/ }).click();
  await page.getByLabel("Orchestration name").fill("Autosave race");
  await firstSaveRequested;
  await page.getByRole("button", { name: "Add Timer node" }).click();
  releaseFirstSave();

  await expect.poll(() => savedBodies.length).toBeGreaterThanOrEqual(2);
  expect(savedBodies[1].nodes.some((node: any) => node.kind === "wait")).toBe(true);
  await expect(page.getByLabel("Orchestration name")).toHaveValue("Autosave race");
  await expect(
    page.getByText("timer wait", { exact: true }).filter({ visible: true }).first()
  ).toBeVisible();
});

test("published-only orchestrations open as read-only inspectable snapshots", async ({
  page,
  apiFixture,
}) => {
  const published = {
    ...draft,
    status: "published",
    version: 2,
    published_at_ms: 1_700_000_000_000,
    nodes: draft.nodes.map((node) =>
      node.kind === "workflow"
        ? { ...node, pinned_definition_hash: "sha256:published-definition" }
        : node
    ),
  };
  apiFixture.mockResponse("/api/engine/orchestrations", {
    orchestrations: [
      {
        orchestration_id: draft.orchestration_id,
        name: draft.name,
        draft: null,
        latest_published_version: 2,
        published_versions: [{ version: 2, published_at_ms: published.published_at_ms }],
      },
    ],
    count: 1,
  });
  apiFixture.mockResponse(`/api/engine/orchestrations/${draft.orchestration_id}`, {
    orchestration_id: draft.orchestration_id,
    draft: null,
    latest_published: published,
  });
  apiFixture.mockResponse(`/api/engine/orchestrations/${draft.orchestration_id}/versions`, {
    orchestration_id: draft.orchestration_id,
    versions: [{ version: 2, name: draft.name, published_at_ms: published.published_at_ms }],
    count: 1,
  });
  apiFixture.mockResponse("/api/engine/automations/v2", { automations: [], count: 0 });
  apiFixture.mockResponse("/api/engine/goals", { goals: [], count: 0 });

  await page.goto("/#/orchestrations");
  await waitForRoute(page, "orchestrations");
  await page.getByRole("button", { name: /Plan and execute/ }).click();
  await expect(page.getByText("Published snapshot", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Orchestration name")).toHaveAttribute("readonly", "");
  await expect(page.getByText("Valid", { exact: true })).toBeVisible();
  await expect(page.getByText("0 issues", { exact: true })).toHaveCount(0);
  await expect(page.getByLabel("Orchestration graph editor")).toBeVisible();
  await expect(page.getByRole("button", { name: "Publish", exact: true })).toBeDisabled();
  await page.getByRole("tab", { name: "Outline" }).click();
  await expect(page.getByRole("button", { name: /Move Plan up/ })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Add transition" })).toBeDisabled();
  expect(
    apiFixture.requests.filter(
      (request) => request === `POST /api/engine/orchestrations/${draft.orchestration_id}/validate`
    )
  ).toHaveLength(0);
});

test("failed autosave pauses retries and preserves recovery actions", async ({
  page,
  apiFixture,
}) => {
  mockOrchestration(apiFixture);
  let attempts = 0;
  await page.route(`**/api/engine/orchestrations/${draft.orchestration_id}`, async (route) => {
    if (route.request().method() !== "PUT") return route.fallback();
    attempts += 1;
    await route.fulfill({
      status: 503,
      contentType: "application/json",
      body: JSON.stringify({ error: "temporarily_unavailable" }),
    });
  });

  await page.goto("/#/orchestrations");
  await waitForRoute(page, "orchestrations");
  await page.getByRole("button", { name: /Plan and execute/ }).click();
  await page.getByLabel("Orchestration name").fill("Unsaved title");
  await expect(page.getByRole("button", { name: "Retry save" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Save as copy" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Reload server copy" })).toBeVisible();
  const attemptsAfterFailure = attempts;
  await page.waitForTimeout(2_600);
  expect(attempts).toBe(attemptsAfterFailure);
  await expect(page.getByLabel("Orchestration name")).toHaveValue("Unsaved title");
});

test("stale publish revisions enter recoverable conflict state", async ({ page, apiFixture }) => {
  mockOrchestration(apiFixture);
  await page.route(
    `**/api/engine/orchestrations/${draft.orchestration_id}/publish`,
    async (route) => {
      await route.fulfill({
        status: 409,
        contentType: "application/json",
        body: JSON.stringify({
          error: "draft_concurrency_conflict",
          detail: "draft concurrency conflict",
        }),
      });
    }
  );

  await page.goto("/#/orchestrations");
  await waitForRoute(page, "orchestrations");
  await page.getByRole("button", { name: /Plan and execute/ }).click();
  const publish = page.getByRole("button", { name: "Publish", exact: true });
  await expect(publish).toBeEnabled();
  page.once("dialog", (dialog) => dialog.accept());
  await publish.click();

  await expect(page.getByText("Save conflict", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Save as copy" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Reload server copy" })).toBeVisible();
  await expect(publish).toBeDisabled();
  await expect(page.getByRole("button", { name: "Archive draft" })).toBeDisabled();
});

test("unpinned workflow references can be refreshed before publishing", async ({
  page,
  apiFixture,
}) => {
  mockOrchestration(apiFixture);
  apiFixture.mockResponse(`/api/engine/orchestrations/${draft.orchestration_id}/stale-references`, {
    orchestration_id: draft.orchestration_id,
    references: [
      {
        node_id: "plan",
        automation_id: "automation-plan",
        state: "unpinned",
        pinned_hash: null,
        current_hash: "sha256:current",
      },
    ],
    stale_count: 0,
  });
  let refreshed = false;
  await page.route(
    `**/api/engine/orchestrations/${draft.orchestration_id}/refresh-references`,
    async (route) => {
      refreshed = true;
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          orchestration: {
            ...draft,
            nodes: draft.nodes.map((node) =>
              node.kind === "workflow"
                ? { ...node, pinned_definition_hash: "sha256:current" }
                : node
            ),
          },
          refreshed_node_ids: ["plan"],
        }),
      });
    }
  );

  await page.goto("/#/orchestrations");
  await waitForRoute(page, "orchestrations");
  await page.getByRole("tab", { name: "Stale" }).click();
  await expect(page.getByRole("button", { name: /Plan and execute/ })).toBeVisible();
  await page.getByRole("button", { name: /Plan and execute/ }).click();
  const refreshButton = page.getByRole("button", { name: "Refresh refs" });
  await expect(refreshButton).toBeEnabled();
  await expect(page.getByRole("button", { name: "Publish", exact: true })).toBeDisabled();
  await refreshButton.click();
  await expect.poll(() => refreshed).toBe(true);
});

test("archived orchestration drafts remain read-only", async ({ page, apiFixture }) => {
  const archived = { ...draft, status: "archived" };
  mockOrchestration(apiFixture);
  apiFixture.mockResponse(`/api/engine/orchestrations/${draft.orchestration_id}`, {
    orchestration_id: draft.orchestration_id,
    draft: archived,
    latest_published: null,
  });

  await page.goto("/#/orchestrations");
  await waitForRoute(page, "orchestrations");
  await page.getByRole("button", { name: /Plan and execute/ }).click();
  await expect(page.getByText("Archived draft", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Orchestration name")).toHaveAttribute("readonly", "");
  await expect(page.getByText("Not checked", { exact: true })).toBeVisible();
  await expect(page.getByText("0 issues", { exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Auto layout" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Publish", exact: true })).toBeDisabled();
  expect(
    apiFixture.requests.filter(
      (request) => request === `POST /api/engine/orchestrations/${draft.orchestration_id}/validate`
    )
  ).toHaveLength(0);
});

test("an orchestration can be reopened from the cached library aggregate", async ({
  page,
  apiFixture,
}) => {
  mockOrchestration(apiFixture);
  await page.goto("/#/orchestrations");
  await waitForRoute(page, "orchestrations");
  await page.getByRole("button", { name: /Plan and execute/ }).click();
  await expect(page.getByLabel("Orchestration graph editor")).toBeVisible();
  await page.getByRole("button", { name: "Back to Orchestrations" }).click();
  await expect(
    page.getByTestId("route-outlet").getByRole("heading", { name: "Orchestrations" })
  ).toBeVisible();
  await page.getByRole("button", { name: /Plan and execute/ }).click();
  await expect(page.getByLabel("Orchestration graph editor")).toBeVisible();
});

for (const [label, viewport] of [
  ["tablet", { width: 1024, height: 768 }],
  ["mobile", { width: 412, height: 915 }],
] as const) {
  test(`orchestration authoring avoids page overflow at ${label} width`, async ({
    page,
    apiFixture,
  }) => {
    mockOrchestration(apiFixture);
    await page.setViewportSize(viewport);
    await page.goto("/#/orchestrations");
    await waitForRoute(page, "orchestrations");
    await page.getByRole("button", { name: /Plan and execute/ }).click();
    await expect(page.getByLabel("Orchestration graph editor")).toBeVisible();
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - window.innerWidth
    );
    expect(overflow).toBeLessThanOrEqual(1);
    await page.getByRole("tab", { name: "Outline" }).click();
    await expect(page.getByRole("tabpanel")).toBeVisible();
  });
}
