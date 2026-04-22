---
title: Agent Workflow Operating Manual
description: A compact, agent-facing checklist for discovering MCP tools, creating workflows, importing bundles, scheduling automations, and repairing failures.
---

Use this page when an agent needs to build or revise a Tandem workflow without guessing at hidden tools or undocumented runtime behavior.

## First-time engine setup

If Tandem is not installed or the engine is not authenticated yet, help the user get to a working engine first. Do not jump into workflow compilation until the engine is reachable and the token source is clear.

Use the smallest setup path that matches the user’s environment:

- **CLI only or headless engine:** install the CLI binaries, generate a token, and start `tandem-engine serve`.
- **Packaged control panel:** install the panel, run `tandem panel init`, and use the token the panel writes into its environment.
- **Desktop app:** open the desktop app, verify the local engine/sidecar is running, and confirm the workspace and provider are configured.

If the user is on a clean machine, point them to:

- [Install CLI Binaries](./install-cli-binaries/)
- [Control Panel (Web Admin)](./control-panel/)
- [First Run Checklist](./first-run/)
- [Engine Authentication For Agents](./engine-authentication-for-agents/)

Token rules:

- For direct engine access, use `TANDEM_API_TOKEN` or `--api-token` with `tandem-engine serve`.
- For the packaged control panel flow, use the `TANDEM_CONTROL_PANEL_ENGINE_TOKEN` created by `tandem panel init`.
- Do not scan arbitrary files or shell history for secrets.
- Before workflow work begins, verify the engine with a health check such as `GET /global/health`.

## Operating order

1. Call `mcp_list` first.
2. If the required MCP server or tool is missing, stop and tell the user to add or connect it.
3. Decide whether the task is:
   - a new generated workflow
   - a revision of an existing planner session
   - an import of a shared bundle
   - a repair of a failed run
4. Ask clarifying questions only when the answer changes compilation, scheduling, or tool access.
5. Preview before applying.
6. Apply only when the user or agent has confirmed the result is ready to persist.
7. Schedule recurring work only after the workflow is durable.
8. Repair or recover runs instead of rebuilding the whole workflow when the failure is local.
9. If the task turns into code edits, follow [Coding Tasks With Tandem](./coding-tasks-with-tandem/) for the workspace, diff, and verification loop.

If the task is a governed recursive automation or a Self-Operator loop, also follow [Self-Operator Playbook](./self-operator-playbook/) and [Automation Governance Lifecycle](./reference/governance-lifecycle/).

## What to ask before compiling

Ask only the questions that change the plan:

- Is this one-off or recurring?
- Should the result become a saved workflow session, a runnable automation, or both?
- Which MCP servers are allowed?
- Should failures pause, retry, degrade, or continue?
- Is this a fresh workflow, an import, a fork, or a repair?
- Should schedule data stay staged until apply?

If the answer requires an unavailable MCP, stop and say so.

## Build and revise

Use the planner loop when the intent is still fuzzy:

- `POST /workflow-plans/preview`
- `POST /workflow-plans/chat/start`
- `POST /workflow-plans/chat/message`
- `POST /workflow-plans/apply`

When revising, preserve the existing session and refine the draft instead of starting over.

## Import and reopen

Use imports when the workflow already exists as a bundle.

1. Preview the bundle with `POST /workflow-plans/import/preview`.
2. Check `import_validation.compatible`.
3. Import with `POST /workflow-plans/import`.
4. Reopen the stored planner session from the Workflow Center.
5. Revise the imported draft before applying it.

Imported workflows are durable planner sessions, not runnable automations yet.

## Schedule

Only schedule after the workflow is durable.

- One-off work should stay manual or run once.
- Recurring work should use the automation schedule fields.
- Imported schedules should be treated as staged until apply.

## Repair and recover

Use repair surfaces when the run failed locally:

- recover paused or blocked runs
- repair the broken node or subtree
- inspect the run checkpoint and failure metadata

Do not throw away a valid workflow just because one stage failed.

## Provenance

Agents should preserve and read provenance:

- source kind
- source bundle digest
- planner session id
- current plan id
- automation id
- run id

For the schema-level governance model behind those records, see [Governance Reference](./reference/governance/).
For the concrete review and pause state machine, see [Automation Governance Lifecycle](./reference/governance-lifecycle/).

If you are helping a user find an older workflow, search from the Workflow Center first.

## Minimal example

```typescript
const inventory = await client.mcp.list();
if (!inventory?.servers?.some((server: any) => server.name === "slack")) {
  throw new Error("Add the Slack MCP before compiling this workflow.");
}

const preview = await client.workflowPlans.chatStart({
  prompt: "Create a recurring workflow that summarizes failed runs and posts them to Slack.",
  planSource: "agent_workflow_manual",
  workspaceRoot: "/workspace/repos/tandem",
});

const applied = await client.workflowPlans.apply({
  planId: preview.plan.plan_id!,
  creatorId: "workflow-agent",
});
```

## What not to do

- Do not invent tools that are not in `mcp_list`.
- Do not apply a workflow before validating it.
- Do not arm a recurring schedule on import without showing the user.
- Do not rebuild a failed workflow from scratch when repair or recover is enough.

## Related Docs

- [Self-Operator Playbook](./self-operator-playbook/)
- [MCP Capability Discovery And Request Flow](./mcp-capability-discovery-and-request-flow/)
- [Automation Governance Lifecycle](./reference/governance-lifecycle/)
