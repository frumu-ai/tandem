import { describe, expect, it } from "vitest";
import { TandemClient } from "../src/client.js";
import type { GoalHandoffTransitionResponse } from "../src/public/index.js";

interface FetchCall { url: string; init?: RequestInit }
const client = () =>
  new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });
const installFetch = (calls: FetchCall[]): (() => void) => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async (input, init) => {
    calls.push({ url: String(input), init });
    return new Response(JSON.stringify({ goal: {}, events: [] }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }) as typeof fetch;
  return () => { globalThis.fetch = originalFetch; };
};

describe("canonical /goals contracts", () => {
  it("uses the exact transition commit literals emitted by the server", () => {
    type CommittedTransition = Extract<GoalHandoffTransitionResponse, { outcome: "committed" }>;
    const commits: CommittedTransition["commit"][] = ["Committed", "AlreadyCommitted"];
    expect(commits).toEqual(["Committed", "AlreadyCommitted"]);
  });

  it("lists and starts goals with canonical query and body fields", async () => {
    const calls: FetchCall[] = [];
    const restore = installFetch(calls);
    try {
      const api = client().statefulRuntime;
      await api.listGoals({ limit: 25, status: "waiting", orchestrationId: "orch/one" });
      await api.startGoal({
        orchestrationId: "orch-one",
        orchestrationVersion: 4,
        objective: "Ship",
        idempotencyKey: "goal-start-1",
        metadata: { release: "1.2.3" },
      });
      expect(calls[0]?.url).toBe(
        "http://localhost:39731/goals?limit=25&status=waiting&orchestration_id=orch%2Fone"
      );
      expect(calls[1]?.url).toBe("http://localhost:39731/goals");
      expect(JSON.parse(String(calls[1]?.init?.body))).toEqual({
        orchestration_id: "orch-one",
        orchestration_version: 4,
        objective: "Ship",
        idempotency_key: "goal-start-1",
        metadata: { release: "1.2.3" },
      });
    } finally { restore(); }
  });

  it("routes aggregate reads and cursor event pages", async () => {
    const calls: FetchCall[] = [];
    const restore = installFetch(calls);
    try {
      const api = client().statefulRuntime;
      await api.getGoal("goal/one");
      await api.getGoalGraph("goal/one");
      await api.listGoalRuns("goal/one");
      await api.listGoalEvents("goal/one", { cursor: 41, limit: 100 });
      await api.listGoalHandoffs("goal/one");
      await api.listGoalWaits("goal/one");
      await api.listGoalArtifacts("goal/one");
      await api.getGoalBudgets("goal/one");
      const base = "http://localhost:39731/goals/goal%2Fone";
      expect(calls.map(({ url }) => url)).toEqual([
        base,
        `${base}/graph`,
        `${base}/runs`,
        `${base}/events?cursor=41&limit=100`,
        `${base}/handoffs`,
        `${base}/waits`,
        `${base}/artifacts`,
        `${base}/budgets`,
      ]);
    } finally { restore(); }
  });

  it("emits transitions, decides handoffs, settles completion, and resolves waits", async () => {
    const calls: FetchCall[] = [];
    const restore = installFetch(calls);
    try {
      const api = client().statefulRuntime;
      await api.emitGoalHandoff("goal/one", {
        transitionKey: "ready",
        artifact: { artifact_type: "release", value: { tag: "v1" } },
        idempotencyKey: "emit-1",
      });
      await api.decideGoalHandoff("goal/one", "handoff/one", {
        decision: "approve",
        reason: "Reviewed",
      });
      await api.settleGoalCompletion("goal/one", {
        transitionKey: "complete",
        finalArtifact: { artifact_type: "release", value: { tag: "v1" } },
      });
      await api.getGoalWait("goal/one", "wait/one");
      await api.resolveGoalWait("goal/one", "wait/one", {
        idempotencyKey: "resolve-1",
        payload: { approved: true },
      });
      const base = "http://localhost:39731/goals/goal%2Fone";
      expect(calls.map(({ url, init }) => [url, init?.method])).toEqual([
        [`${base}/transitions`, "POST"],
        [`${base}/handoffs/handoff%2Fone/decision`, "POST"],
        [`${base}/completion`, "POST"],
        [`${base}/waits/wait%2Fone`, undefined],
        [`${base}/waits/wait%2Fone/resolve`, "POST"],
      ]);
      expect(JSON.parse(String(calls[1]?.init?.body))).toEqual({
        decision: "approve", reason: "Reviewed",
      });
    } finally { restore(); }
  });

  it("posts lifecycle controls without alternate pinned-version fields", async () => {
    const calls: FetchCall[] = [];
    const restore = installFetch(calls);
    try {
      const api = client().statefulRuntime;
      await api.cancelGoal("goal/one", { reason: "Stop" });
      await api.pauseGoal("goal/one", { reason: "Review" });
      await api.resumeGoal("goal/one");
      const base = "http://localhost:39731/goals/goal%2Fone";
      expect(calls.map(({ url, init }) => [url, init?.method, JSON.parse(String(init?.body))])).toEqual([
        [`${base}/cancel`, "POST", { reason: "Stop" }],
        [`${base}/pause`, "POST", { reason: "Review" }],
        [`${base}/resume`, "POST", {}],
      ]);
    } finally { restore(); }
  });

  it("unwraps cursor SSE events, skips ready, and handles split CRLF frames", async () => {
    const originalFetch = globalThis.fetch;
    let requestedUrl = "";
    globalThis.fetch = (async (input) => {
      requestedUrl = String(input);
      const encoder = new TextEncoder();
      const body = new ReadableStream({
        start(controller) {
          controller.enqueue(encoder.encode('event: ready\r\ndata: {"goal_id":"goal/one","cursor":11}\r\n\r\n'));
          controller.enqueue(encoder.encode('id: 12\r\nevent: orchestration.goal.paused\r\ndata: {"cursor":12,"event":{"schema_version":1,"event_id":"e12","goal_seq":12,'));
          controller.enqueue(encoder.encode('"seq":12,"event_type":"orchestration.goal.paused","occurred_at_ms":300,"run_id":"run-one","payload":{"reason":"Review"}}}\r\n\r\n'));
          controller.close();
        },
      });
      return new Response(body, { status: 200, headers: { "Content-Type": "text/event-stream" } });
    }) as typeof fetch;
    try {
      const next = await client().statefulRuntime.events("goal/one", { cursor: 11 }).next();
      expect(requestedUrl).toBe("http://localhost:39731/goals/goal%2Fone/events/stream?cursor=11");
      expect(next.value).toMatchObject({
        type: "orchestration.goal.paused",
        cursor: 12,
        goal_seq: 12,
        runId: "run-one",
        properties: { reason: "Review" },
      });
    } finally { globalThis.fetch = originalFetch; }
  });
});
