import { expect, test, waitForRoute, type ApiFixture } from "./fixtures/api";

const projection = {
  goal: {
    schema_version: 1,
    goal_id: "goal-live-1",
    orchestration_id: "orch-live-1",
    orchestration_version: 4,
    objective: "Ship the governed release",
    status: "active",
    tenant_context: { org_id: "org-e2e", workspace_id: "workspace-e2e", source: "explicit" },
    policy: { max_hops: 12, on_limit: "pause_for_review" },
    current_node_id: "execute",
    hop_count: 3,
    total_tokens: 1840,
    total_cost_usd: 0.42,
    created_at_ms: 1_700_000_000_000,
    updated_at_ms: 1_700_000_004_000,
  },
  orchestration: {
    schema_version: 1,
    orchestration_id: "orch-live-1",
    name: "Governed release",
    status: "published",
    version: 4,
    root_node_id: "plan",
    nodes: [
      { node_id: "plan", name: "Plan", kind: "workflow", automation_id: "plan", position: { x: 40, y: 100 }, allowed_transition_keys: ["next"] },
      { node_id: "execute", name: "Execute", kind: "workflow", automation_id: "execute", position: { x: 350, y: 100 }, allowed_transition_keys: ["complete"] },
      { node_id: "done", name: "Done", kind: "terminal", outcome: "complete", position: { x: 660, y: 100 } },
    ],
    edges: [
      { edge_id: "plan-execute", from_node_id: "plan", to_node_id: "execute", transition_key: "next" },
      { edge_id: "execute-done", from_node_id: "execute", to_node_id: "done", transition_key: "complete" },
    ],
    goal_policy: { max_hops: 12, on_limit: "pause_for_review" },
    tenant_context: { org_id: "org-e2e", workspace_id: "workspace-e2e", source: "explicit" },
    created_at_ms: 1_700_000_000_000,
    updated_at_ms: 1_700_000_001_000,
  },
  graph: {
    available: true,
    nodes: [
      { node_id: "plan", name: "Plan", kind: "workflow", state: "completed", runs: [{ run_id: "run-plan", status: "completed", hop_index: 1 }] },
      { node_id: "execute", name: "Execute", kind: "workflow", state: "running", runs: [{ run_id: "run-execute", status: "running", hop_index: 2 }] },
      { node_id: "done", name: "Done", kind: "terminal", state: "not_started", runs: [] },
    ],
    edges: [
      { edge: { edge_id: "plan-execute", from_node_id: "plan", to_node_id: "execute", transition_key: "next" }, state: "taken" },
      { edge: { edge_id: "execute-done", from_node_id: "execute", to_node_id: "done", transition_key: "complete" }, state: "eligible" },
    ],
  },
  workflow: {
    automation_id: "execute",
    run_id: "run-execute",
    status: "running",
    stage: "Verification",
    checkpoint: { completed_nodes: ["build"], pending_nodes: ["verify"], blocked_nodes: [] },
    outputs: { tests: "passing" },
    retries: { attempts: {}, verdicts: {} },
  },
  waits: [],
  handoffs: [],
  artifacts: [],
  budgets: { policy: { max_hops: 12, on_limit: "pause_for_review" }, consumed: { hops: 3, total_tokens: 1840, total_cost_usd: 0.42 }, remaining: { hops: 9, tokens: 8160, cost_usd: 4.58 } },
  timeline: {
    events: [
      { cursor: 1, event: { event_id: "evt-1", goal_seq: 1, event_type: "goal_started", occurred_at_ms: 1_700_000_000_000, schema_version: 1, run_id: "run-plan", seq: 1, scope: {}, payload: {} } },
      { cursor: 2, event: { event_id: "evt-2", goal_seq: 2, event_type: "workflow_started", occurred_at_ms: 1_700_000_002_000, schema_version: 1, run_id: "run-execute", seq: 2, scope: {}, payload: {} } },
    ],
    count: 2,
    limit: 120,
    truncated: false,
  },
  cursor: 2,
  live_cursor: 2,
  retained_from_cursor: 1,
  mode: "live",
  schema_version: 1,
  goal_id: "goal-live-1",
  historical_state: { source: "current_goal", exact: true },
  orchestration_source: "goal_metadata_snapshot",
  recovery: {
    resume_plan: { plan_id: "resume-after-verification", next_node_id: "execute", ready: true },
    uncertain_effects: [{ effect: "release notification", status: "unverified" }],
    receipts: [{ receipt_id: "receipt-17", status: "recorded" }],
    recovery_status: "awaiting_operator",
  },
  actions: [
    { id: "pause", kind: "pause", label: "Pause goal", enabled: true, impact: "Stops new work after the current checkpoint." },
    { id: "cancel", kind: "cancel", label: "Cancel goal", enabled: true, destructive: true, reason_required: true, impact: "Cancels active runtime resources." },
    {
      id: "retry",
      kind: "retry",
      label: "Retry step",
      enabled: true,
      impact: "Retries a failed step with operator-selected scope.",
      payload_fields: [
        { name: "step_id", label: "Failed step", required: true },
        { name: "scope", label: "Retry scope", required: true, options: ["step", { value: "workflow", label: "Whole workflow" }] },
      ],
    },
    {
      id: "resume-plan",
      kind: "resume_plan",
      label: "Apply resume plan",
      enabled: true,
      impact: "Resumes from a reviewed recovery plan.",
      payload_fields: [
        { name: "plan_id", label: "Resume plan", required: true },
        { name: "receipt_policy", label: "Receipt policy", required: false, options: ["verify", "accept"] },
      ],
    },
  ],
};

function mockGoalProjection(apiFixture: ApiFixture) {
  apiFixture.mockResponse(`/api/engine/goals/${projection.goal.goal_id}/projection`, projection);
}

test("goal picker loads the selected goal without remounting the route", async ({ page, apiFixture }) => {
  apiFixture.mockResponse("/api/engine/goals", { goals: [projection.goal], count: 1 });
  mockGoalProjection(apiFixture);
  await page.goto("/#/goal-operations");
  await waitForRoute(page, "goal-operations");

  await page.getByRole("button", { name: new RegExp(projection.goal.objective) }).click();

  await expect(page).toHaveURL(new RegExp(`goal_id=${projection.goal.goal_id}`));
  await expect(page.getByRole("heading", { name: projection.goal.objective })).toBeVisible();
});

test("goal operations preserves selection and disables governed actions in replay", async ({ page, apiFixture }) => {
  mockGoalProjection(apiFixture);
  await page.goto(`/#/goal-operations?goal_id=${projection.goal.goal_id}`);
  await waitForRoute(page, "goal-operations");
  await expect(page.getByRole("heading", { name: projection.goal.objective })).toBeVisible();
  await expect(page.getByLabel("Read-only goal orchestration canvas")).toBeVisible();
  await expect(page.getByRole("status").filter({ hasText: /Live connection|Polling fallback/ })).toBeVisible();

  await page.getByText("Execute", { exact: true }).first().click();
  await expect(page.getByRole("heading", { name: "Execute" })).toBeVisible();
  await expect(page.getByText("Verification", { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Resume Plan" })).toBeVisible();
  await expect(page.getByText("resume-after-verification", { exact: false })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Uncertain Effects" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Receipts" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Recovery Status" })).toBeVisible();
  await expect(page.getByText("awaiting_operator", { exact: true })).toBeVisible();
  await page.waitForTimeout(3_300);
  await expect(page.locator(".goal-ops-node.selected")).toContainText("Execute");

  await page.getByRole("button", { name: "Replay" }).click();
  await expect(page.getByRole("button", { name: "Pause goal" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Cancel goal" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Retry step" })).toBeDisabled();
  await expect(page.getByRole("button", { name: "Apply resume plan" })).toBeDisabled();
  await expect(page.getByText("Disabled in replay")).toBeVisible();

  const canvasBox = await page.getByTestId("goal-operations-canvas").boundingBox();
  const inspectorBox = await page.getByLabel("Goal execution inspector").boundingBox();
  expect(canvasBox).not.toBeNull();
  expect(inspectorBox).not.toBeNull();
  if (page.viewportSize()!.width <= 640) {
    expect(inspectorBox!.y).toBeGreaterThanOrEqual(canvasBox!.y + canvasBox!.height - 2);
    expect(canvasBox!.width).toBeGreaterThan(300);
  } else {
    expect(inspectorBox!.x).toBeGreaterThanOrEqual(canvasBox!.x + canvasBox!.width - 2);
    expect(canvasBox!.height).toBeGreaterThan(400);
  }
});

test("governed retry and resume-plan confirmations submit structured payload", async ({ page, apiFixture }) => {
  mockGoalProjection(apiFixture);
  await page.goto(`/#/goal-operations?goal_id=${projection.goal.goal_id}`);
  await waitForRoute(page, "goal-operations");

  await page.getByRole("button", { name: "Retry step" }).click();
  const retryFields = page.getByRole("group", { name: "Retry step details" });
  const retryConfirm = page.locator(".tcp-confirm-dialog").getByRole("button", { name: "Retry step" });
  await expect(retryFields.getByLabel("Failed step (required)")).toBeVisible();
  await expect(retryFields.getByLabel("Retry scope (required)")).toHaveValue("step");
  await expect(retryConfirm).toBeDisabled();
  await retryFields.getByLabel("Failed step (required)").fill("verify");
  await retryFields.getByLabel("Retry scope (required)").selectOption("workflow");
  const retryRequest = page.waitForRequest((request) => request.url().endsWith("/actions/retry") && request.method() === "POST");
  await retryConfirm.click();
  expect((await retryRequest).postDataJSON().payload).toEqual({ step_id: "verify", scope: "workflow" });

  await page.getByRole("button", { name: "Apply resume plan" }).click();
  const resumeFields = page.getByRole("group", { name: "Apply resume plan details" });
  await resumeFields.getByLabel("Resume plan (required)").fill("resume-after-verification");
  await resumeFields.getByLabel("Receipt policy").selectOption("verify");
  const resumeRequest = page.waitForRequest((request) => request.url().endsWith("/actions/resume-plan") && request.method() === "POST");
  await page.locator(".tcp-confirm-dialog").getByRole("button", { name: "Apply resume plan" }).click();
  expect((await resumeRequest).postDataJSON().payload).toEqual({
    plan_id: "resume-after-verification",
    receipt_policy: "verify",
  });
});

test("replay waits for the canonical historical projection", async ({ page, apiFixture }) => {
  mockGoalProjection(apiFixture);
  await page.goto(`/#/goal-operations?goal_id=${projection.goal.goal_id}`);
  await waitForRoute(page, "goal-operations");
  await expect(page.getByLabel("Read-only goal orchestration canvas")).toBeVisible();

  const historicalRequest = apiFixture.holdNext(
    `/api/engine/goals/${projection.goal.goal_id}/projection`,
    "GET"
  );
  await page.getByRole("button", { name: "Replay" }).click();
  await historicalRequest.waitUntilRequested();
  await expect(page.getByRole("heading", { name: "Loading goal operations" })).toBeVisible();
  await expect(page.getByLabel("Read-only goal orchestration canvas")).toHaveCount(0);

  historicalRequest.release();
  await expect(page.getByLabel("Read-only goal orchestration canvas")).toBeVisible();
});

test("replay never presents live data when historical projection is unavailable", async ({ page, apiFixture }) => {
  mockGoalProjection(apiFixture);
  await page.goto(`/#/goal-operations?goal_id=${projection.goal.goal_id}`);
  await waitForRoute(page, "goal-operations");
  await expect(page.getByLabel("Read-only goal orchestration canvas")).toBeVisible();

  await page.route("**/projection?cursor=**", (route) => route.fulfill({
    status: 503,
    contentType: "application/json",
    body: JSON.stringify({ error: "historical_projection_unavailable" }),
  }));
  await page.getByRole("button", { name: "Replay" }).click();

  await expect(page.getByRole("heading", { name: "Goal projection unavailable" })).toBeVisible();
  await expect(page.getByLabel("Read-only goal orchestration canvas")).toHaveCount(0);
});
