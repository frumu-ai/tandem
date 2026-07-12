const DAY_MS = 24 * 60 * 60 * 1_000;
export const TAN707_START_MS = Date.UTC(2100, 0, 1);
export const tan707Day = (day: number) => TAN707_START_MS + day * DAY_MS;

const goal = {
  schema_version: 1,
  goal_id: "goal-tan-707",
  orchestration_id: "tan-707-goal-loop",
  orchestration_version: 1,
  objective: "Deliver and verify the 180-day program",
  status: "active",
  tenant_context: { org_id: "local", workspace_id: "local", source: "local_implicit" },
  policy: {
    max_hops: 6,
    deadline_at_ms: tan707Day(180),
    max_total_tokens: 20_000,
    max_total_cost_usd: 5,
    on_limit: "pause_for_review",
  },
  active_run_id: "run-verify",
  current_node_id: "verify",
  hop_count: 2,
  total_tokens: 8_400,
  total_cost_usd: 1.75,
  created_at_ms: tan707Day(0),
  updated_at_ms: tan707Day(179),
};

const orchestration = {
  schema_version: 1,
  orchestration_id: "tan-707-goal-loop",
  name: "Goal Plan Execute Verify Replan",
  status: "published",
  version: 1,
  root_node_id: "plan",
  nodes: [
    { node_id: "plan", name: "Plan", kind: "workflow", automation_id: "tan-707-plan", position: { x: 20, y: 100 }, allowed_transition_keys: ["planned"] },
    { node_id: "execute", name: "Execute", kind: "workflow", automation_id: "tan-707-execute", position: { x: 290, y: 100 }, allowed_transition_keys: ["executed"] },
    { node_id: "verify", name: "Verify", kind: "workflow", automation_id: "tan-707-verify", position: { x: 560, y: 100 }, allowed_transition_keys: ["complete", "replan"] },
    { node_id: "complete", name: "Complete", kind: "terminal", outcome: "complete", position: { x: 830, y: 100 } },
  ],
  edges: [
    { edge_id: "plan-execute", from_node_id: "plan", to_node_id: "execute", transition_key: "planned" },
    { edge_id: "execute-verify", from_node_id: "execute", to_node_id: "verify", transition_key: "executed" },
    { edge_id: "verify-complete", from_node_id: "verify", to_node_id: "complete", transition_key: "complete" },
    { edge_id: "verify-replan", from_node_id: "verify", to_node_id: "plan", transition_key: "replan" },
  ],
  goal_policy: goal.policy,
  tenant_context: goal.tenant_context,
  created_at_ms: tan707Day(0),
  updated_at_ms: tan707Day(0),
};

const events = [
  [1, "stateful_runtime.goal.started", "run-plan", 0],
  [2, "stateful_runtime.handoff.committed", "run-execute", 1],
  [3, "stateful_runtime.wait.created", "run-execute", 30],
  [4, "stateful_runtime.wait.created", "run-execute", 60],
  [5, "stateful_runtime.wait.created", "run-execute", 90],
  [6, "stateful_runtime.tool_effect.unknown", "run-execute", 120],
  [7, "stateful_runtime.handoff.committed", "run-verify", 179],
].map(([cursor, eventType, runId, day]) => ({
  cursor,
  event: {
    schema_version: 1,
    event_id: `tan707-event-${cursor}`,
    goal_seq: cursor,
    seq: cursor,
    event_type: eventType,
    occurred_at_ms: tan707Day(Number(day)),
    run_id: runId,
    scope: {},
    payload: { virtual_day: day },
  },
}));

const common = {
  schema_version: 1,
  goal_id: goal.goal_id,
  goal,
  orchestration,
  graph: {
    available: true,
    nodes: [
      { node_id: "plan", name: "Plan", kind: "workflow", state: "completed", runs: [{ run_id: "run-plan", status: "completed", hop_index: 0 }] },
      { node_id: "execute", name: "Execute", kind: "workflow", state: "completed", runs: [{ run_id: "run-execute", status: "completed", hop_index: 1 }] },
      { node_id: "verify", name: "Verify", kind: "workflow", state: "running", runs: [{ run_id: "run-verify", status: "running", hop_index: 2 }] },
      { node_id: "complete", name: "Complete", kind: "terminal", state: "not_started", runs: [] },
    ],
    edges: orchestration.edges.map((edge) => ({
      edge,
      state: edge.edge_id === "verify-complete" || edge.edge_id === "verify-replan" ? "eligible" : "taken",
    })),
  },
  workflow: {
    automation_id: "tan-707-verify",
    run_id: "run-verify",
    status: "running",
    stage: "Verify",
    checkpoint: { completed_nodes: ["plan", "execute"], pending_nodes: ["verify"], blocked_nodes: [] },
    outputs: { named_outcomes: ["complete", "replan"] },
    retries: { attempts: {}, verdicts: {} },
  },
  waits: [
    { wait_id: "wait-day-30-timer", run_id: "run-execute", wait_kind: "timer", status: "woken", created_at_ms: tan707Day(30), updated_at_ms: tan707Day(30), scope: {} },
    { wait_id: "wait-day-60-approval", run_id: "run-execute", wait_kind: "approval", status: "woken", created_at_ms: tan707Day(60), updated_at_ms: tan707Day(60), scope: {} },
    { wait_id: "wait-day-90-webhook", run_id: "run-execute", wait_kind: "webhook", status: "woken", created_at_ms: tan707Day(90), updated_at_ms: tan707Day(90), scope: {} },
    { wait_id: "wait-day-120-external", run_id: "run-execute", wait_kind: "external_condition", status: "waiting", created_at_ms: tan707Day(120), updated_at_ms: tan707Day(120), scope: {} },
  ],
  handoffs: [
    { handoff_id: "handoff-planned", transition_key: "planned", source_run_id: "run-plan", status: "consumed" },
    { handoff_id: "handoff-executed", transition_key: "executed", source_run_id: "run-execute", status: "consumed" },
  ],
  artifacts: [],
  budgets: {
    policy: goal.policy,
    consumed: { hops: 2, total_tokens: 8_400, total_cost_usd: 1.75 },
    remaining: { hops: 4, tokens: 11_600, cost_usd: 3.25, deadline_ms: DAY_MS },
  },
  recovery: {
    resume_plan: { plan_id: "reconcile-day-120-effect", next_node_id: "verify", ready: false },
    uncertain_effects: [{ effect_id: "effect-day-120-release", operation: "mcp.release.notify", status: "unknown" }],
    receipts: [],
    recovery_status: "awaiting_operator",
  },
  actions: [
    { id: "pause", kind: "pause", label: "Pause goal", enabled: true },
    { id: "cancel", kind: "cancel", label: "Cancel goal", enabled: true, destructive: true, reason_required: true },
  ],
  orchestration_source: "goal_metadata_snapshot",
  retained_from_cursor: 1,
};

export const tan707LiveProjection = {
  ...common,
  mode: "live",
  cursor: 7,
  live_cursor: 7,
  historical_state: { source: "current_goal", exact: true },
  timeline: { events, count: events.length, limit: 240, truncated: false },
};

export const tan707ReplayProjection = {
  ...common,
  mode: "replay",
  cursor: 4,
  live_cursor: 7,
  historical_state: { source: "projection_snapshot", exact: true },
  timeline: { events: events.slice(0, 4), count: 4, limit: 240, truncated: false },
};
