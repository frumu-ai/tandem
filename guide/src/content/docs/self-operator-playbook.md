---
title: Self-Operator Playbook
description: LLM-facing operating guide for governed recursive automation authoring, capability requests, and weekly report publishing.
---

Use this guide when an agent is responsible for inspecting Tandem, identifying one gap, authoring a new automation to close it, and publishing a report without bypassing governance.

This is the "how to operate what we just built" doc. If you only read one page, read this one first.

## Read First

Before you start acting, refresh your working context from the canonical docs:

- [Governance Reference](./reference/governance/)
- [Automation Governance Lifecycle](./reference/governance-lifecycle/)
- [MCP Automated Agents](./mcp-automated-agents/)
- [Agent Workflow Operating Manual](./agent-workflow-operating-manual/)
- [Build an Automation With the AI Assistant](./automation-composer-workflows/)
- [Scheduling Workflows And Automations](./sdk/scheduling-automations/)
- [Tools Reference](./reference/tools/)

If you are missing the current tool names or approval semantics, do not guess. Re-read the docs or inspect the engine inventory first.

## The Operating Loop

Follow this order for a weekly Self-Operator run:

1. Refresh Tandem context from the docs above.
2. Inventory runtime MCPs with `mcp_list`.
3. Inventory the catalog overlay with `mcp_list_catalog`.
4. Decide whether the gap is:
   - already connected and enabled
   - cataloged but not connected
   - uncataloged and needs a capability request
5. Read recent strategic context from the relevant connected source, such as Notion.
6. Produce one deterministic gap analysis.
7. If the gap requires human approval, file an approval request and stop.
8. If the gap is actionable now, author the automation.
9. Write the weekly report as a durable artifact.
10. Only publish externally if the automation has explicit approval to do so.

## Step 1: Refresh Context

The Self-Operator is not a free-form planner. It should ground itself in the Tandem docs that describe current runtime behavior.

Read for:

- provenance and lineage rules
- capability grants and approval queues
- MCP discovery order
- automation authoring and scheduling paths
- tool allowlists and execution policy

If the docs disagree with the engine, trust the engine and treat the docs as stale until updated.

## Step 2: Inventory What Exists

Use `mcp_list` first.

That is the runtime truth for:

- connected MCP servers
- discovered tools
- what the current run can actually call

Then use `mcp_list_catalog` to answer the next question:

- what Tandem knows about the broader catalog
- what is connected versus merely cataloged
- what is visible but disabled
- what is uncataloged and therefore requires a human decision

Do not collapse those two inventories into one mental bucket. The distinction is the point.

## Step 3: Classify The Gap

For each candidate capability, decide which of these applies:

| State                     | Meaning                                       | Next move                  |
| ------------------------- | --------------------------------------------- | -------------------------- |
| connected + enabled       | You can act on it now                         | Use the tool path directly |
| connected + disabled      | The connector exists but is not usable        | Ask a human to enable it   |
| cataloged + not connected | Tandem knows about it, but it is not wired in | Ask a human to connect it  |
| uncataloged               | Tandem does not have a catalog entry          | File a capability request  |

If the gap is uncataloged, use `mcp_request_capability`. Do not silently switch to a different connector or invent a tool.

## Step 4: Read Strategic Context

Read only the sources that matter for the report you are writing.

For a weekly strategic loop, a good default is:

- the last 14 days of relevant Notion context
- the current automation inventory
- recent governance reviews
- recent approval decisions

Keep the context bounded. The goal is not maximum recall. The goal is a reliable decision.

## Step 5: Make One Deterministic Decision

Your reasoning output should be structured enough that the next step is mechanical.

Recommended shape:

```json
{
  "gap_type": "connected_missing_tool | cataloged_not_connected | uncataloged | quota | depth | spend | lifecycle",
  "blocked": true,
  "recommended_action": "use_existing_tool | request_connection | request_capability | request_quota_override | author_automation | stop",
  "target": {
    "mcp_name": "notion",
    "tool_name": "list_databases"
  },
  "rationale": "why this gap matters",
  "evidence": ["doc paths, tool ids, or source ids"],
  "confidence": 0.0
}
```

If the result is approval-bound, stop after filing the request. Do not continue into automation authoring until the human has responded.

## Step 6: File The Right Approval

Use the approval queue when the action needs human review.

Common cases:

- request a new MCP capability
- request a quota override
- request a recursion-depth override
- request an external-post approval
- request elevated capability for `creates_agents` or `modifies_grants`
- request a retirement or extension action for an expiring automation

The canonical write surface is the approval queue. The request itself should be explicit and auditable.

For MCP gaps, `mcp_request_capability` is the agent-facing entrypoint.

## Step 7: Author The Automation

When the gap is actionable and approved, prefer `automationsV2.create`.

Use planner chat or the AI Composer only when the shape is still fuzzy.

When you create or patch an automation, include:

- explicit provenance through the request identity
- bounded schedule semantics
- a small tool allowlist
- an explicit MCP policy
- declared capabilities only if they are approved
- a retirement or expiration policy if the automation should not live forever

Do not create a recursive automation that can create agents or modify grants unless the relevant capability grant has already been approved.

## Step 8: Publish The Report

Write the report as a durable artifact before you try to post or notify anyone.

Good report shape:

- what context was inspected
- what gap was found
- which MCPs were available
- whether a request was filed
- what automation was created or updated
- what remains blocked

If the report includes any external side effect, route that through approval first.

## Step 9: Respond To Governance Interruptions

If the automation shows `review_required`, `paused_for_lifecycle`, or a related review kind, stop mutating it and inspect the governance record.

Relevant signals include:

- `review_kind = dependency_revoked`
- `review_kind = creation_quota`
- `review_kind = run_drift`
- `review_kind = health_drift`
- `review_kind = expiration_soon`
- `review_kind = expired`
- `review_kind = retired`

Treat these as operator coordination states, not as minor warnings.

If a grant was revoked or an MCP policy was narrowed, the automation may already be paused. Approval clears the review record, but the run still needs the normal resume or re-arm path if the automation remains paused.

## What Not To Do

- Do not invent MCP tools that are not in `mcp_list` or `mcp_list_catalog`.
- Do not connect a new MCP yourself.
- Do not self-grant capability or quota exceptions.
- Do not keep creating automations once a creation quota or review threshold trips.
- Do not treat a paused automation as healthy just because the review was approved.
- Do not skip the report artifact and leave only chat output.

## Related Docs

- [MCP Capability Discovery And Request Flow](./mcp-capability-discovery-and-request-flow/)
- [Automation Governance Lifecycle](./reference/governance-lifecycle/)
- [MCP Automated Agents](./mcp-automated-agents/)
- [Agent Workflow Operating Manual](./agent-workflow-operating-manual/)
- [Automation Examples For Teams](./automation-examples-for-teams/)
