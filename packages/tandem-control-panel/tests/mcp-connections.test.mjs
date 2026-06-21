import assert from "node:assert/strict";
import test from "node:test";

import {
  mcpConnectionOwnerLabel,
  normalizeMcpConnectionGrants,
  normalizeMcpConnectionsFromInventory,
} from "../src/features/mcp/mcpConnections.ts";

test("normalizes tenant-scoped MCP connections from server inventory", () => {
  const rows = normalizeMcpConnectionsFromInventory({
    github: {
      name: "github",
      connections: [
        {
          connection_id: "github:alice",
          server_id: "github",
          connection_class: "user_owned",
          connected: true,
          enabled: true,
          owner: { type: "human_actor", actor_id: "alice" },
          upstream_account: { email: "alice@example.com" },
          tenant_context: {
            org_id: "org-a",
            workspace_id: "workspace-a",
            actor_id: "alice",
          },
          tool_cache: [
            {
              namespaced_name: "mcp.github.list_issues",
              tool_name: "list_issues",
              input_schema: { secret: "ignored by frontend normalizer" },
            },
          ],
        },
      ],
    },
    slack: {
      name: "slack",
      connections: [
        {
          connectionId: "slack:shared",
          server: "slack",
          connectionClass: "shared_read_write",
          connected: false,
          enabled: true,
          owner: { type: "shared_connection", grant_id: "marketing-shared" },
          toolCache: ["mcp.slack.send_message"],
        },
      ],
    },
  });

  assert.equal(rows.length, 2);
  assert.equal(rows[0].server, "github");
  assert.equal(rows[0].connectionId, "github:alice");
  assert.equal(mcpConnectionOwnerLabel(rows[0]), "alice@example.com");
  assert.deepEqual(rows[0].toolCache, ["mcp.github.list_issues"]);
  assert.equal(rows[1].connectionClass, "shared_read_write");
  assert.deepEqual(rows[1].toolCache, ["mcp.slack.send_message"]);
});

test("deduplicates MCP connection grants by server, connection, and run_as", () => {
  const grants = normalizeMcpConnectionGrants([
    { server: "github", connection_id: "github:alice" },
    { server: "github", connectionId: "github:alice" },
    { server_name: "slack", connection_id: "slack:shared", run_as: { mode: "delegated" } },
  ]);

  assert.deepEqual(grants, [
    { server: "github", connection_id: "github:alice" },
    { server: "slack", connection_id: "slack:shared", run_as: { mode: "delegated" } },
  ]);
});
