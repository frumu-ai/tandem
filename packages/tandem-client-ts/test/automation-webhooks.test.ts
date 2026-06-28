import { describe, expect, it } from "vitest";
import { TandemClient } from "../src/client.js";

describe("Automation webhook SDK coverage", () => {
  it("normalizes camelCase trigger payload fields before sending", async () => {
    const client = new TandemClient({
      baseUrl: "http://localhost:39731",
      token: "test-token",
    });
    const originalFetch = globalThis.fetch;
    const bodies: unknown[] = [];

    globalThis.fetch = (async (_input, init) => {
      bodies.push(JSON.parse(String(init?.body || "{}")));
      return new Response(
        JSON.stringify({
          trigger: {
            trigger_id: "trigger-1",
            automation_id: "automation-1",
            name: "GitHub issues",
            provider: "github",
            enabled: true,
          },
        }),
        { status: 200, headers: { "Content-Type": "application/json" } }
      );
    }) as typeof fetch;

    try {
      await client.automationsV2.createWebhookTrigger("automation-1", {
        provider: "github",
        providerEventKind: "issues.opened",
        defaultDataClass: "customer_data",
        defaultRiskTier: "internal_write",
        owningOrgUnitId: "support",
        resourceScope: { root: { resource_id: "automation-project" } },
      });
      await client.automationsV2.updateWebhookTrigger("automation-1", "trigger-1", {
        providerEventKind: null,
        defaultDataClass: "internal",
        defaultRiskTier: null,
      });

      expect(bodies[0]).toMatchObject({
        provider: "github",
        provider_event_kind: "issues.opened",
        default_data_class: "customer_data",
        default_risk_tier: "internal_write",
        owning_org_unit_id: "support",
        resource_scope: { root: { resource_id: "automation-project" } },
      });
      expect(bodies[0]).not.toHaveProperty("providerEventKind");
      expect(bodies[0]).not.toHaveProperty("defaultDataClass");
      expect(bodies[0]).not.toHaveProperty("defaultRiskTier");
      expect(bodies[0]).not.toHaveProperty("owningOrgUnitId");
      expect(bodies[0]).not.toHaveProperty("resourceScope");

      expect(bodies[1]).toMatchObject({
        provider_event_kind: null,
        default_data_class: "internal",
        default_risk_tier: null,
      });
      expect(bodies[1]).not.toHaveProperty("providerEventKind");
      expect(bodies[1]).not.toHaveProperty("defaultDataClass");
      expect(bodies[1]).not.toHaveProperty("defaultRiskTier");
    } finally {
      globalThis.fetch = originalFetch;
    }
  });
});
