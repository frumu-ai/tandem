import { describe, expect, it } from "vitest";
import type { AutomationV2FlowNode, AutomationWaitSpec } from "../src/public/index.js";

describe("Automation V2 wait-node contracts", () => {
  it("represents a correlated webhook wait without tool execution fields", () => {
    const wait: AutomationWaitSpec = {
      kind: "webhook",
      trigger_id: "trigger-review-complete",
      correlation: {
        field: "provider_event_id",
        value: { source: "node_output", node_id: "prepare", json_pointer: "/event_id" },
      },
      timeout: { expires_after_ms: 86_400_000, on_timeout: "escalate", escalate_to: "ops" },
    };
    const node: AutomationV2FlowNode = {
      nodeId: "wait-for-review",
      agentId: "",
      objective: "Wait for the review webhook",
      dependsOn: ["prepare"],
      wait,
    };

    expect(node.wait?.kind).toBe("webhook");
    expect("toolPolicy" in node).toBe(false);
  });
});
