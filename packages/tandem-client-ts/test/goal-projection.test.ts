import { describe, expect, it } from "vitest";
import { TandemClient } from "../src/client.js";

describe("goal projection SDK contracts", () => {
  it("serializes projection cursors and governed action input", async () => {
    const originalFetch = globalThis.fetch;
    const calls: Array<{ url: string; init?: RequestInit }> = [];
    globalThis.fetch = (async (input, init) => {
      calls.push({ url: String(input), init });
      return new Response(JSON.stringify({}), {
        status: 200,
        headers: { "Content-Type": "application/json" },
      });
    }) as typeof fetch;

    try {
      const runtime = new TandemClient({
        baseUrl: "http://localhost:39731",
        token: "test-token",
      }).statefulRuntime;
      await runtime.getGoalProjection("goal/one", { cursor: 184, limit: 75 });
      await runtime.performGoalAction("goal/one", "approve/handoff", {
        expectedUpdatedAtMs: 1_765_430_400_000,
        idempotencyKey: "action-1",
        reason: "Reviewed",
        decision: "approve",
        payload: { handoff_id: "handoff-1" },
      });

      expect(calls[0]?.url).toBe(
        "http://localhost:39731/goals/goal%2Fone/projection?cursor=184&limit=75"
      );
      expect(calls[1]?.url).toBe(
        "http://localhost:39731/goals/goal%2Fone/actions/approve%2Fhandoff"
      );
      expect(calls[1]?.init?.method).toBe("POST");
      expect(JSON.parse(String(calls[1]?.init?.body))).toEqual({
        expected_updated_at_ms: 1_765_430_400_000,
        idempotency_key: "action-1",
        reason: "Reviewed",
        decision: "approve",
        payload: { handoff_id: "handoff-1" },
      });
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});
