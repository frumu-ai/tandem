import { afterEach, describe, expect, it, vi } from "vitest";
import { TandemClient } from "../src/client.js";

const DAY_MS = 24 * 60 * 60 * 1_000;
const START_MS = Date.UTC(2100, 0, 1);
const day = (value: number) => START_MS + value * DAY_MS;
const goalId = "goal/tan-707";
const base = `http://localhost:39731/goals/${encodeURIComponent(goalId)}`;

const goal = {
  schema_version: 1,
  goal_id: goalId,
  orchestration_id: "tan-707-goal-loop",
  orchestration_version: 1,
  objective: "Deliver and verify the 180-day program",
  status: "active",
  tenant_context: { org_id: "local", workspace_id: "local", source: "local_implicit" },
  policy: {
    max_hops: 6,
    deadline_at_ms: day(180),
    max_total_tokens: 20_000,
    max_total_cost_usd: 5,
    on_limit: "pause_for_review",
  },
  active_run_id: "run-verify",
  current_node_id: "verify",
  hop_count: 2,
  total_tokens: 8_400,
  total_cost_usd: 1.75,
  created_at_ms: day(0),
  updated_at_ms: day(179),
};

const waits = ["timer", "approval", "webhook", "external_condition"].map((wait_kind, index) => ({
  schema_version: 1,
  wait_id: `wait/${wait_kind}`,
  run_id: "run-execute",
  wait_kind,
  status: index === 3 ? "waiting" : "woken",
  scope: { schema_version: 1, tenant_context: goal.tenant_context },
  created_at_ms: day(30 * (index + 1)),
  updated_at_ms: day(30 * (index + 1)),
}));

const projection = (mode: "live" | "replay", cursor: number) => ({
  schema_version: 1,
  goal_id: goalId,
  goal,
  orchestration: {
    orchestration_id: "tan-707-goal-loop",
    name: "Goal Plan Execute Verify Replan",
    status: "published",
    version: 1,
    root_node_id: "plan",
    nodes: [
      { node_id: "plan", name: "Plan", kind: "workflow", automation_id: "plan" },
      { node_id: "execute", name: "Execute", kind: "workflow", automation_id: "execute" },
      { node_id: "verify", name: "Verify", kind: "workflow", automation_id: "verify", allowed_transition_keys: ["complete", "replan"] },
      { node_id: "complete", name: "Complete", kind: "terminal", outcome: "complete" },
    ],
    edges: [
      { edge_id: "verify-complete", from_node_id: "verify", to_node_id: "complete", transition_key: "complete" },
      { edge_id: "verify-replan", from_node_id: "verify", to_node_id: "plan", transition_key: "replan" },
    ],
    goal_policy: goal.policy,
    tenant_context: goal.tenant_context,
    created_at_ms: day(0),
    updated_at_ms: day(0),
  },
  graph: { available: true, nodes: [], edges: [] },
  workflow: { automation_id: "verify", run_id: "run-verify", status: "running" },
  waits,
  handoffs: [],
  artifacts: [],
  budgets: {
    policy: goal.policy,
    consumed: { hops: 2, total_tokens: 8_400, total_cost_usd: 1.75 },
    remaining: { hops: 4, tokens: 11_600, cost_usd: 3.25, deadline_ms: DAY_MS },
  },
  timeline: { events: [], count: 0, limit: 240, truncated: false },
  cursor,
  live_cursor: 7,
  retained_from_cursor: 1,
  mode,
  historical_state: { source: mode === "live" ? "current_goal" : "projection_snapshot", exact: true },
  orchestration_source: "goal_metadata_snapshot",
  recovery: {
    uncertain_effects: [{ effect_id: "effect-day-120-release", status: "unknown" }],
    recovery_status: "awaiting_operator",
  },
  actions: [],
});

afterEach(() => vi.unstubAllGlobals());

describe("TAN-707 180-day goal inspection", () => {
  it("preserves canonical live/replay state and all durable wait kinds", async () => {
    const requested: string[] = [];
    vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
      const url = String(input);
      requested.push(url);
      if (url === `${base}/projection`) return Response.json(projection("live", 7));
      if (url === `${base}/projection?cursor=4&limit=240`) {
        return Response.json(projection("replay", 4));
      }
      if (url === `${base}/waits`) {
        return Response.json({ goal_id: goalId, waits, count: waits.length });
      }
      if (url === `${base}/budgets`) {
        return Response.json({ goal_id: goalId, status: "active", budgets: projection("live", 7).budgets });
      }
      throw new Error(`unexpected request: ${url}`);
    });

    const runtime = new TandemClient({ baseUrl: "http://localhost:39731" }).statefulRuntime;
    const live = await runtime.getGoalProjection(goalId);
    const replay = await runtime.getGoalProjection(goalId, { cursor: 4, limit: 240 });
    const durableWaits = await runtime.listGoalWaits(goalId);
    const budgets = await runtime.getGoalBudgets(goalId);

    expect(replay.mode).toBe("replay");
    expect(replay.historical_state).toEqual({ source: "projection_snapshot", exact: true });
    for (const key of ["goal", "graph", "workflow", "waits", "handoffs", "budgets"] as const) {
      expect(replay[key]).toEqual(live[key]);
    }
    expect(new Set(durableWaits.waits.map((item) => item.wait_kind))).toEqual(
      new Set(["timer", "approval", "webhook", "external_condition"])
    );
    expect(budgets.budgets.remaining.hops).toBe(4);
    expect(live.goal.policy.deadline_at_ms - live.goal.created_at_ms).toBe(180 * DAY_MS);
    expect(live.orchestration.edges.map((edge) => edge.transition_key)).toEqual(["complete", "replan"]);
    expect(requested).toEqual([
      `${base}/projection`,
      `${base}/projection?cursor=4&limit=240`,
      `${base}/waits`,
      `${base}/budgets`,
    ]);
  });
});
