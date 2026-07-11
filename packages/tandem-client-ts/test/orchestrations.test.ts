import { describe, expect, it } from "vitest";
import { TandemClient } from "../src/client.js";

interface FetchCall { url: string; init?: RequestInit }

const client = () =>
  new TandemClient({ baseUrl: "http://localhost:39731", token: "test-token" });

const installFetch = (calls: FetchCall[]): (() => void) => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = (async (input, init) => {
    calls.push({ url: String(input), init });
    return new Response(JSON.stringify({ orchestration: {}, updated_at_ms: 8 }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }) as typeof fetch;
  return () => { globalThis.fetch = originalFetch; };
};

describe("canonical draft-v0 orchestration contracts", () => {
  it("lists, creates, and gets aggregate state", async () => {
    const calls: FetchCall[] = [];
    const restore = installFetch(calls);
    try {
      const api = client().orchestrations;
      await api.list({ status: "draft", limit: 25 });
      await api.create({
        orchestration_id: "orch-new",
        name: "New orchestration",
        root_node_id: "start",
        nodes: [],
        goal_policy: { max_hops: 12 },
      });
      await api.get("orch/one");

      expect(calls.map(({ url, init }) => [url, init?.method])).toEqual([
        ["http://localhost:39731/orchestrations?status=draft&limit=25", undefined],
        ["http://localhost:39731/orchestrations", "POST"],
        ["http://localhost:39731/orchestrations/orch%2Fone", undefined],
      ]);
      expect(JSON.parse(String(calls[1]?.init?.body))).toEqual({
        orchestration_id: "orch-new",
        name: "New orchestration",
        root_node_id: "start",
        nodes: [],
        edges: [],
        goal_policy: { max_hops: 12 },
      });
    } finally { restore(); }
  });

  it("uses canonical draft actions, stale references, versions, and dry-run", async () => {
    const calls: FetchCall[] = [];
    const restore = installFetch(calls);
    try {
      const api = client().orchestrations;
      await api.updateDraft("orch/one", {
        name: "Release train",
        root_node_id: "build",
        expected_updated_at_ms: 8,
      });
      await api.archive("orch/one");
      await api.validate("orch/one");
      await api.staleReferences("orch/one");
      await api.refreshReferences("orch/one", 9);
      await api.publish("orch/one");
      await api.listVersions("orch/one");
      await api.getVersion("orch/one", 2);
      await api.dryRun("orch/one", {
        fromNodeId: "build",
        transitionKey: "ready",
        artifactType: "release",
        version: 2,
      });

      const base = "http://localhost:39731/orchestrations/orch%2Fone";
      expect(calls.map(({ url, init }) => [url, init?.method])).toEqual([
        [base, "PUT"],
        [`${base}/archive`, "POST"],
        [`${base}/validate`, "POST"],
        [`${base}/stale-references`, undefined],
        [`${base}/refresh-references`, "POST"],
        [`${base}/publish`, "POST"],
        [`${base}/versions`, undefined],
        [`${base}/versions/2`, undefined],
        [`${base}/dry-run`, "POST"],
      ]);
      expect(JSON.parse(String(calls[0]?.init?.body))).toMatchObject({
        expected_updated_at_ms: 8,
      });
      expect(JSON.parse(String(calls[4]?.init?.body))).toEqual({ expected_updated_at_ms: 9 });
      expect(JSON.parse(String(calls[8]?.init?.body))).toEqual({
        from_node_id: "build",
        transition_key: "ready",
        artifact_type: "release",
        version: 2,
      });
    } finally { restore(); }
  });
});
