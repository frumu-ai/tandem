---
title: MCP Capability Discovery And Request Flow
description: LLM-facing guide for reasoning about connected MCPs, cataloged MCPs, and approval-bound capability requests.
---

Use this guide when an agent needs to answer a capability question safely:

- what MCPs are available right now
- what MCPs Tandem knows about but has not connected
- what MCPs are uncataloged and therefore need a human decision
- how to ask for a new capability without pretending it already exists

This page is intentionally narrow. It is about discovery and request flow, not execution policy.

## The Four States

Treat every MCP as belonging to one of four states:

| State                     | Meaning                                      | Can execute tools?    | What the agent should do   |
| ------------------------- | -------------------------------------------- | --------------------- | -------------------------- |
| connected + enabled       | The server is wired in and usable            | Yes, if policy allows | Use the tool path directly |
| connected + disabled      | The server exists but is turned off          | No                    | Ask a human to enable it   |
| cataloged + not connected | Tandem knows about it but it is not wired in | No                    | Ask a human to connect it  |
| uncataloged               | Tandem has no catalog entry for it           | No                    | File a capability request  |

Do not merge catalog visibility with execution access. Tandem keeps those separate on purpose.

## Discovery Order

Use this order:

1. Call `mcp_list`.
2. If you need more context for gap analysis, call `mcp_list_catalog`.
3. If you already know the remote server and need exact tool names, use `GET /mcp/tools` and `GET /tool/ids`.
4. If the connector is cataloged but not connected, ask a human to connect it.
5. If the connector is uncataloged, call `mcp_request_capability`.
6. Only after the connector is available and authorized should you plan to execute its tools.

If `mcp_list` does not show the needed server, do not guess. If `mcp_list_catalog` also does not show it, the request belongs in the approval queue.

## What `mcp_list_catalog` Returns

The catalog overlay is the agent-facing bridge between "what Tandem knows" and "what Tandem can use right now".

The response shape includes:

```json
{
  "servers": [],
  "overlay": {
    "inventory_version": 1,
    "connected_server_names": [],
    "enabled_server_names": [],
    "connected_catalog_server_names": [],
    "connected_disabled_server_names": [],
    "cataloged_not_connected_server_names": [],
    "cataloged_disabled_server_names": [],
    "uncataloged_connected_servers": [],
    "status_counts": {
      "connected_enabled": 0,
      "connected_disabled": 0,
      "cataloged_not_connected": 0,
      "cataloged_disabled": 0,
      "uncataloged_connected": 0
    }
  }
}
```

Use the overlay to separate three questions:

- Can I act on it now?
- Is it known but disconnected?
- Is it unknown and therefore approval-bound?

## How To Use `mcp_request_capability`

`mcp_request_capability` is the agent-facing way to record a capability gap.

Required fields:

```json
{
  "agent_id": "self_operator",
  "mcp_name": "notion",
  "rationale": "Need Notion database access to read strategic context for the weekly report"
}
```

Optional fields:

- `catalog_slug`
- `requested_tools`
- `context`
- `expires_at_ms`

The tool does not connect the server and does not execute any remote tool.

It returns an approval object:

```json
{
  "ok": true,
  "approval": {
    "approval_id": "apr_...",
    "request_type": "capability_request",
    "requested_by": { "kind": "agent" },
    "target_resource": { "type": "agent", "id": "self_operator" },
    "rationale": "...",
    "context": { "...": "..." },
    "status": "pending",
    "expires_at_ms": 0,
    "reviewed_by": null,
    "reviewed_at_ms": null,
    "review_notes": null
  },
  "requested_by": { "kind": "agent" }
}
```

Use that response as the durable proof that the gap was recorded.

## How To Reason About A Gap

When you discover a missing capability, classify it before you act:

```json
{
  "state": "connected_enabled | connected_disabled | cataloged_not_connected | uncataloged",
  "gap": "tool_missing | connector_missing | connector_disabled | unknown",
  "available_tools": ["..."],
  "next_action": "use_tool | ask_human_to_connect | ask_human_to_enable | file_capability_request | stop",
  "approval_needed": true,
  "reason": "short explanation"
}
```

That output should be deterministic enough that the next step is obvious.

## What Not To Do

- Do not invent a connector because the task would be easier if it existed.
- Do not call remote MCP tools through `mcp_debug` unless you already know the exact URL and tool name.
- Do not treat `mcp_list` as a catalog search endpoint.
- Do not self-connect a new MCP.
- Do not self-grant execution access to an MCP that is visible in the catalog but not enabled.
- Do not continue into automation authoring if the capability you need is still approval-bound.

## Related Docs

- [Self-Operator Playbook](./self-operator-playbook/)
- [Automation Governance Lifecycle](./reference/governance-lifecycle/)
- [MCP Automated Agents](./mcp-automated-agents/)
- [Tools Reference](./reference/tools/)
