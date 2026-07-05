---
title: Automation Examples for Teams
description: Real examples that prove Tandem can build reliable, governed, repeated workflows from a prompt or via SDK/code.
---

Tandem workflows are designed to be authored in three ways with the same runtime:

- a low-friction prompt in the control-panel automation wizard,
- a prompt-first conversation in the AI Composer tab,
- direct engine SDK calls for deterministic deployment,
- and planned task prompts that generate the same structure automatically.

## Agent quickindex (copy first)

- [TypeScript SDK: automationsV2 examples](../sdk/typescript/)
- [Python SDK: automations_v2 examples](../sdk/python/)
- [Control-panel path: automation wizard + schedule](./control-panel/)
- [AI-first workflow composer](./automation-composer-workflows/)
- [Automation V2 Webhooks](./automation-v2-webhooks/)
- [Tandem Wow Demo Playbook](./tandem-wow-demo-playbook/)

The same runtime means the automation behavior is identical, no matter how it was authored.

## 1) The easiest path: AI Composer or automation wizard in the control panel

Use this when you want to show that Tandem can turn a conversational prompt into a governed automation before the user ever touches JSON.

Example prompt:

```text
Build a governed automation named "Todo digest + notify" for /workspace/repos/my-repo.
Use a file-reading step to find TODO and FIXME items under src/ and docs/.
Write docs/todo_digest.md with path, line number, and severity.
End with an MCP step that sends a short Slack summary and includes the report path.
```

What this demonstrates:

- policy-safe multi-agent automation creation
- a final MCP step as the terminating action
- previewable payloads that map cleanly to `automationsV2.create`
- `runNow` plus standard run-history inspection

Use this when you want a fast proof that non-engineers can ship autonomous ops.

1. Open **Automations** and start the **Wizard**.
2. Choose **Workflow** (or the equivalent automated workflow starter).
3. Paste a bounded prompt with an explicit output contract.
4. Review the generated nodes and policies.
5. Run once, then schedule if the first run is successful.

Example prompt that works well as a first wizard pass:

```text
Build an automation named "Todo digest + notify" for workspace "/workspace/repos/my-repo".
Use a two-node flow:
- Node 1 (Reader): find TODO/FIXME entries under src/ and docs/.
  - output: docs/todo_digest.md
  - include file path and line number for each finding
  - rank findings as high/medium/low risk
- Node 2 (Notifier): send a short summary to Slack and include that docs/todo_digest.md was updated
  - only post in #team-ops
  - keep message under 12 lines

Set the schedule to manual for now.
Before recurring runs, show the generated DAG for review.
```

What the engine receives is an authored graph with explicit objectives and tool policies, so run and scale it like any production automation.

## 2) Deterministic path: create, run, and inspect with the TypeScript SDK

Use this when you already know the exact DAG and want repeatability with version control.

```ts
const created = await client.automationsV2.create({
  name: "Todo digest + notify",
  status: "active",
  schedule: {
    type: "manual",
    timezone: "UTC",
    misfire_policy: { type: "run_once" },
  },
  workspace_root: "/workspace/repos/my-repo",
  agents: [
    {
      agent_id: "reader",
      display_name: "Reader",
      skills: [],
      tool_policy: { allowlist: ["read", "write"] },
      mcp_policy: { allowed_servers: [], allowed_tools: [] },
      approval_policy: "auto",
    },
    {
      agent_id: "notifier",
      display_name: "Notifier",
      skills: [],
      tool_policy: { allowlist: ["read"] },
      mcp_policy: { allowed_servers: ["slack"], allowed_tools: ["send_message"] },
      approval_policy: "auto",
    },
  ],
  flow: {
    nodes: [
      {
        node_id: "collect_todos",
        agent_id: "reader",
        objective: "Find TODO and FIXME items under src/ and docs/ with file + line context.",
      },
      {
        node_id: "write_report",
        agent_id: "reader",
        depends_on: ["collect_todos"],
        objective: "Create docs/todo_digest.md with grouped findings and severity ranking.",
      },
      {
        node_id: "notify_team",
        agent_id: "notifier",
        depends_on: ["write_report"],
        objective: "Use MCP to send a short summary to team and include path docs/todo_digest.md.",
      },
    ],
  },
  creator_id: "demo-operator",
});

const automationId = created.automation?.automation_id;
await client.automationsV2.runNow(automationId);
const runs = await client.automationsV2.listRuns(automationId, 5);
console.log(runs.runs.map((r) => ({ runId: r.run_id, status: r.status })));
```

Python equivalent:

```python
automation = await client.automations_v2.create({
    "name": "Todo digest + notify",
    "status": "active",
    "schedule": {"type": "manual", "timezone": "UTC", "misfire_policy": {"type": "run_once"}},
    "workspace_root": "/workspace/repos/my-repo",
    "agents": [
        {
            "agent_id": "reader",
            "display_name": "Reader",
            "skills": [],
            "tool_policy": {"allowlist": ["read", "write"]},
            "mcp_policy": {"allowed_servers": [], "allowed_tools": []},
            "approval_policy": "auto",
        },
        {
            "agent_id": "notifier",
            "display_name": "Notifier",
            "skills": [],
            "tool_policy": {"allowlist": ["read"]},
            "mcp_policy": {"allowed_servers": ["slack"], "allowed_tools": ["send_message"]},
            "approval_policy": "auto",
        },
    ],
    "flow": {
        "nodes": [
            {
                "node_id": "collect_todos",
                "agent_id": "reader",
                "objective": "Find TODO and FIXME items under src/ and docs/ with file + line context.",
            },
            {
                "node_id": "write_report",
                "agent_id": "reader",
                "depends_on": ["collect_todos"],
                "objective": "Create docs/todo_digest.md with grouped findings and severity ranking.",
            },
            {
                "node_id": "notify_team",
                "agent_id": "notifier",
                "depends_on": ["write_report"],
                "objective": "Use MCP to send a short summary to team and include path docs/todo_digest.md.",
            },
        ]
    },
    "creator_id": "demo-operator",
})

automation_id = automation.automation_id
await client.automations_v2.run_now(automation_id)
runs = await client.automations_v2.list_runs(automation_id, 5)
print([(r.run_id, r.status) for r in runs.runs])
```

This is the “game changer” pattern because one artifact (the automation graph) is portable and can be reviewed, edited, scheduled, repaired, and replayed.

If you prefer Python, mirror this with `client.automations_v2.create(...)` and `run_now(...)`.

## 3) Complex workflow pattern: file-first analysis then final MCP output

This is the exact shape for “read local context, do structured work, then hand off or notify.”

```ts
const created = await client.automationsV2.create({
  name: "Repo risk radar",
  status: "active",
  schedule: {
    type: "interval",
    interval_seconds: 12 * 60 * 60,
    timezone: "UTC",
    misfire_policy: { type: "run_once" },
  },
  workspace_root: "/workspace/repos/my-repo",
  agents: [
    {
      agent_id: "scanner",
      display_name: "Scanner",
      tool_policy: { allowlist: ["read"] },
      mcp_policy: { allowed_servers: [], allowed_tools: [] },
      approval_policy: "auto",
    },
    {
      agent_id: "analyst",
      display_name: "Analyst",
      tool_policy: { allowlist: ["read", "write"] },
      mcp_policy: { allowed_servers: [], allowed_tools: [] },
      approval_policy: "auto",
    },
    {
      agent_id: "notifier",
      display_name: "Notifier",
      tool_policy: { allowlist: ["read"] },
      mcp_policy: { allowed_servers: ["slack"], allowed_tools: ["send_message"] },
      approval_policy: "auto",
    },
  ],
  flow: {
    nodes: [
      {
        node_id: "scan_sources",
        agent_id: "scanner",
        objective:
          "Find TODO/FIXME patterns in src/, docs/, and README files. Output the top 25 findings as JSON payload in working notes.",
      },
      {
        node_id: "build_risk_report",
        agent_id: "analyst",
        depends_on: ["scan_sources"],
        objective:
          "Create docs/todo_digest.md with risk tiers, rationale, and exact file references. Include one 'no_action_needed' branch if nothing is critical.",
      },
      {
        node_id: "notify_and_link",
        agent_id: "notifier",
        depends_on: ["build_risk_report"],
        objective:
          "Call slack.send_message with a short run summary and the path docs/todo_digest.md. If report is critical, add a bold alert marker.",
      },
    ],
  },
});

const run = await client.automationsV2.runNow(created.automation?.automation_id);
const status = await client.automationsV2.getRun(run?.run_id);
console.log({
  runId: run?.run_id,
  status: status?.run?.status,
  artifact: "docs/todo_digest.md",
});
```

Python equivalent:

```python
created = await client.automations_v2.create({
    "name": "Repo risk radar",
    "status": "active",
    "schedule": {
        "type": "interval",
        "interval_seconds": 12 * 60 * 60,
        "timezone": "UTC",
        "misfire_policy": {"type": "run_once"},
    },
    "workspace_root": "/workspace/repos/my-repo",
    "agents": [
        {
            "agent_id": "scanner",
            "display_name": "Scanner",
            "tool_policy": {"allowlist": ["read"]},
            "mcp_policy": {"allowed_servers": [], "allowed_tools": []},
            "approval_policy": "auto",
        },
        {
            "agent_id": "analyst",
            "display_name": "Analyst",
            "tool_policy": {"allowlist": ["read", "write"]},
            "mcp_policy": {"allowed_servers": [], "allowed_tools": []},
            "approval_policy": "auto",
        },
        {
            "agent_id": "notifier",
            "display_name": "Notifier",
            "tool_policy": {"allowlist": ["read"]},
            "mcp_policy": {"allowed_servers": ["slack"], "allowed_tools": ["send_message"]},
            "approval_policy": "auto",
        },
    ],
    "flow": {
        "nodes": [
            {
                "node_id": "scan_sources",
                "agent_id": "scanner",
                "objective": "Find TODO/FIXME patterns in src/, docs/, and README files. Output the top 25 findings as JSON payload in working notes.",
            },
            {
                "node_id": "build_risk_report",
                "agent_id": "analyst",
                "depends_on": ["scan_sources"],
                "objective": "Create docs/todo_digest.md with risk tiers, rationale, and exact file references. Include one 'no_action_needed' branch if nothing is critical.",
            },
            {
                "node_id": "notify_and_link",
                "agent_id": "notifier",
                "depends_on": ["build_risk_report"],
                "objective": "Call slack.send_message with a short run summary and the path docs/todo_digest.md. If report is critical, add a bold alert marker.",
            },
        ]
    },
})

run = await client.automations_v2.run_now(created.automation_id)
status = await client.automations_v2.get_run(run.run_id)
print({
    "run_id": run.run_id,
    "status": status.run.status,
    "artifact": "docs/todo_digest.md",
})
```

This demonstrates all three enterprise requirements in one DAG:

- local file inspection
- deterministic staging artifact creation
- external action through MCP at the end

## 4) Linear repair-loop guard template

Use this pattern when Linear issue webhooks trigger an ACA repair workflow. Linear
signs the delivery, but the workflow still needs a first-node guard because
Linear webhooks can cover more than one project, label, or issue state.

The guard node is the authority boundary. It should produce a small JSON decision
before any ACA, repo, MCP, or write-capable node runs:

```json
{
  "allowed": true,
  "reason_code": "linear_repair_ready",
  "linear_issue_id": "TAN-123",
  "linear_project": "Tandem Native Linear Webhooks",
  "required_label": "tandem:repair-ready",
  "idempotency_key": "linear:TAN-123:repair-ready"
}
```

Recommended first-node prompt:

```text
Inspect the incoming Linear webhook payload from automation_webhook.
Act as the repair-loop guard before ACA receives authority.

Allow the run only when all checks pass:
- provider is "linear"
- issue.project.id or issue.project.name matches the configured repair project
- issue labels include "tandem:repair-ready"
- action/type is one of issue.created, issue.updated, create, or update
- issue state is not canceled, done, archived, or otherwise terminal
- this Linear issue has not already started an active repair run for the same idempotency key

Return a JSON guard decision with:
- allowed: boolean
- reason_code: stable snake_case reason
- linear_issue_id
- linear_project
- matching_labels
- action
- idempotency_key
- human_summary

If any check fails, set allowed=false, do not call ACA or external tools, and
include the suppression reason in reason_code.
```

Use stable reason codes so the delivery/run history is legible:

| Reason code | Meaning |
| --- | --- |
| `linear_repair_ready` | Project, label, action, state, and idempotency checks passed. |
| `linear_project_not_allowed` | The signed event came from a different Linear project. |
| `linear_missing_repair_label` | The issue does not have the configured repair-ready label. |
| `linear_action_not_allowed` | The event action is not one of the configured issue actions. |
| `linear_issue_terminal` | The issue is done, canceled, archived, or otherwise not repairable. |
| `linear_duplicate_repair_run` | A repair run already exists for the same Linear issue/idempotency key. |

Then make the ACA node depend on the guard node and start with:

```text
Continue only if the Linear guard decision has allowed=true.
If allowed=false, summarize the guard reason and stop without using repo or MCP tools.
```

What this demonstrates:

- a signed webhook is still scoped by Tandem policy before agent authority expands
- non-demo Linear project events are accepted or suppressed without starting ACA
- the repair-loop demo can show project and label checks as a boundary, not just a convenience branch
- duplicate issue events do not create multiple repair attempts unless the operator intentionally reruns

For direct Linear webhook setup, see [Automation V2 Webhooks](./automation-v2-webhooks/#linear-issue-webhooks).

## 5) Make these examples discoverable to agents

To let your agents pull the docs directly instead of carrying snippets in prompts:

- publish this `guide/src/content/docs` content in your internal docs host, and
- expose that docs source as an MCP-readable source for agents, then point agents at the automation example path.

Then agents can read the same canonical source before compiling new workflows, which avoids stale examples and keeps agent behavior consistent with your team’s standards.

Recommended links for implementation path:

- [Control Panel (Web Admin)](./control-panel/)
- [SDK scheduling guide](./sdk/scheduling-automations/)
- [Prompting Workflows And Missions](./prompting-workflows-and-missions/)
- [Creating And Running Workflows And Missions](./creating-and-running-workflows-and-missions/)
- [MCP Automated Agents](./mcp-automated-agents/)
