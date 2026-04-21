---
title: TypeScript SDK
description: "@frumu/tandem-client — Node.js client for the Tandem engine"
---

## Install

```bash
npm install @frumu/tandem-client
```

Requires **Node.js 18+** (uses native `fetch` and `ReadableStream`).

For recurring jobs and scheduled automations, see [Scheduling Workflows And Automations](./scheduling-automations/).

For agent-focused automation authoring, also start here:

- [Automation Examples For Teams](../automation-examples-for-teams/) — practical TypeScript + Python examples.
- [Build an Automation With the AI Assistant](../automation-composer-workflows/) — prompt-first authoring and clarification flow.
- [Agent Workflow And Mission Quickstart](../agent-workflow-mission-quickstart/)
- [Creating And Running Workflows And Missions](../creating-and-running-workflows-and-missions/)

## Agent quick links in this page

Use these when you just want copy-paste blocks:

- Simple DAG example + immediate run checks: `Todo digest + notify` in the **AutomationsV2** section.
- Complex file-to-artifact-to-MCP workflow: `Repo risk radar` in the **AutomationsV2** section.

## Engine prerequisite

The SDK talks to a running `tandem-engine` over HTTP/SSE. Install and start the engine first:

```bash
npm install -g @frumu/tandem
tandem-engine serve --api-token "$(tandem-engine token generate)"
```

Then pass the same token into `new TandemClient({ baseUrl, token })`.

## Quick start

```typescript
import { TandemClient } from "@frumu/tandem-client";

const client = new TandemClient({
  baseUrl: "http://localhost:39731",
  token: "your-engine-token", // tandem-engine token generate
});

// 1. Create a session
const sessionId = await client.sessions.create({
  title: "My agent",
  directory: "/path/to/project",
});

// 2. Start an async run
const { runId } = await client.sessions.promptAsync(
  sessionId,
  "Summarize the README and list the top 3 TODOs"
);

// 3. Stream the response
for await (const event of client.stream(sessionId, runId)) {
  if (event.type === "session.response") {
    process.stdout.write(String(event.properties.delta ?? ""));
  }
  if (
    event.type === "run.complete" ||
    event.type === "run.completed" ||
    event.type === "run.failed" ||
    event.type === "session.run.finished"
  ) {
    break;
  }
}
```

## TandemClient

```typescript
new TandemClient({ baseUrl, token, timeoutMs? })
```

### Top-level methods

| Method                                   | Returns                       | Description                      |
| ---------------------------------------- | ----------------------------- | -------------------------------- |
| `health()`                               | `SystemHealth`                | Check engine readiness           |
| `stream(sessionId, runId?)`              | `AsyncGenerator<EngineEvent>` | Stream events from an active run |
| `globalStream()`                         | `AsyncGenerator<EngineEvent>` | Stream all engine events         |
| `runEvents(runId, { sinceSeq?, tail? })` | `EngineEvent[]`               | Pull stored run events           |
| `listToolIds()`                          | `string[]`                    | List all tool IDs                |
| `listTools()`                            | `ToolSchema[]`                | List tools with full schemas     |
| `executeTool(tool, args?)`               | `ToolExecuteResult`           | Execute a tool directly          |

---

### `client.sessions`

| Method                                                          | Description                           |
| --------------------------------------------------------------- | ------------------------------------- |
| `create({ title?, directory?, provider?, model? })`             | Create a session, returns `sessionId` |
| `list({ q?, page?, pageSize?, archived?, scope?, workspace? })` | List sessions                         |
| `get(sessionId)`                                                | Get session details                   |
| `update(sessionId, { title?, archived? })`                      | Update title or archive status        |
| `archive(sessionId)`                                            | Archive a session                     |
| `delete(sessionId)`                                             | Permanently delete a session          |
| `messages(sessionId)`                                           | Get full message history              |
| `todos(sessionId)`                                              | Get pending TODOs                     |
| `activeRun(sessionId)`                                          | Get the currently active run          |
| `promptAsync(sessionId, prompt)`                                | Start async run → `{ runId }`         |
| `promptSync(sessionId, prompt)`                                 | Blocking prompt → reply text          |
| `abort(sessionId)`                                              | Abort the active run                  |
| `cancel(sessionId)`                                             | Cancel the active run                 |
| `cancelRun(sessionId, runId)`                                   | Cancel a specific run                 |
| `fork(sessionId)`                                               | Fork into a child session             |
| `diff(sessionId)`                                               | Get workspace diff from last run      |
| `revert(sessionId)`                                             | Revert uncommitted changes            |
| `unrevert(sessionId)`                                           | Undo a revert                         |
| `children(sessionId)`                                           | List forked child sessions            |
| `summarize(sessionId)`                                          | Trigger conversation summarization    |
| `attach(sessionId, targetWorkspace)`                            | Re-attach to a different workspace    |

#### Prompt with file parts

Use raw engine route when you need mixed `parts` payloads:

```typescript
const res = await fetch(`/session/${encodeURIComponent(sessionId)}/prompt_async?return=run`, {
  method: "POST",
  headers: {
    "content-type": "application/json",
    Authorization: `Bearer ${token}`,
  },
  body: JSON.stringify({
    parts: [
      {
        type: "file",
        mime: "text/markdown",
        filename: "audit.md",
        url: "/srv/tandem/channel_uploads/control-panel/audit.md",
      },
      { type: "text", text: "Summarize this file." },
    ],
  }),
});
const run = await res.json();
```

`file` part shape:

- `type`: `"file"`
- `mime`: MIME type string
- `filename`: optional display filename
- `url`: HTTP URL, local path, or `file://...`

### `client.permissions`

```typescript
const { requests } = await client.permissions.list();
for (const req of requests) {
  await client.permissions.reply(req.id, "always"); // "allow" | "always" | "deny" | "once"
}
```

### `client.questions`

```typescript
const { questions } = await client.questions.list();
for (const q of questions) {
  await client.questions.reply(q.id, "yes");
  // or: await client.questions.reject(q.id);
}
```

### `client.providers`

```typescript
const catalog = await client.providers.catalog();
await client.providers.setDefaults("openrouter", "anthropic/claude-3.7-sonnet");
await client.providers.setApiKey("openrouter", "sk-or-...");
const status = await client.providers.authStatus();
```

### `client.identity`

```typescript
const identity = await client.identity.get();

await client.identity.patch({
  identity: {
    bot: { canonical_name: "Ops Assistant" },
    personality: {
      default: {
        preset: "concise",
        custom_instructions: "Prioritize deployment safety and rollback clarity.",
      },
    },
  },
});
```

Built-in presets include: `balanced`, `concise`, `friendly`, `mentor`, `critical`.

### `client.channels`

```typescript
await client.channels.put("discord", {
  bot_token: "bot:xxx",
  guild_id: "1234567890",
  security_profile: "public_demo",
});
const status = await client.channels.status();
const config = await client.channels.config();
const prefs = await client.channels.toolPreferences("discord");
await client.channels.setToolPreferences("discord", {
  disabled_tools: ["webfetch_html"],
});
const verification = await client.channels.verify("discord");

console.log(status.discord.connected);
console.log(config.discord.securityProfile);
console.log(prefs.enabled_tools);
console.log(verification.ok);
```

### `client.mcp`

```typescript
await client.mcp.add({
  name: "arcade",
  transport: "https://mcp.arcade.ai/mcp",
  allowed_tools: ["search", "search_docs"],
});
await client.mcp.connect("arcade");
const tools = await client.mcp.listTools();
const resources = await client.mcp.listResources();
await client.mcp.patch("arcade", { allowed_tools: ["search"] });
await client.mcp.patch("arcade", { clear_allowed_tools: true });
await client.mcp.setEnabled("arcade", false);
```

### `client.memory`

```typescript
// Store (global record; SDK `text` maps to server `content`)
await client.memory.put({
  text: "The team uses Rust for all backend services.",
  run_id: "run-abc",
});

// Search
const { results } = await client.memory.search({
  query: "backend technology choices",
  limit: 5,
});

// List, promote, demote, delete
const { items } = await client.memory.list({ q: "architecture", userId: "user-123" });
await client.memory.promote({ id: items[0].id! });
await client.memory.demote({ id: items[0].id!, runId: "run-abc" });
await client.memory.delete(items[0].id!);

// Audit
const log = await client.memory.audit({ run_id: "run-abc" });
```

#### Context Memory (L0/L1/L2 layers)

```typescript
// Resolve a URI to a memory node
const { node } = await client.memory.contextResolveUri("tandem://user/user123/memories");

// Get a tree of memory nodes
const { tree } = await client.memory.contextTree("tandem://resources/myproject", { maxDepth: 3 });

// Generate L0/L1 layers for a node
await client.memory.contextGenerateLayers("node-id-123");

// Distill a session conversation into memories
const result = await client.memory.contextDistill("session-abc", [
  "User: I prefer Python over Rust",
  "Assistant: Got it, I'll use Python for this task",
]);
```

### Additional namespaces

The TypeScript SDK also exposes the newer engine surfaces used across the Tandem repo:

- `client.browser` for `status()`, `install()`, and `smokeTest()` host flows
- `client.workflows` for workflow registry, runs, hooks, simulation, and live events
- `client.resources` for key-value resources
- `client.skills` for validation, routing, evals, compile, and generate flows in addition to list/get/import
- `client.packs` and `client.capabilities` for pack lifecycle and capability resolution
- `client.automationsV2`, `client.bugMonitor`, `client.coder`, `client.agentTeams`, `client.missions`, and `client.optimizations` for newer orchestration APIs

```typescript
const browser = await client.browser.status();
const workflows = await client.workflows.list();
const resources = await client.resources.list({ prefix: "agent-config/" });
const catalog = await client.skills.catalog();
```

For actual browser automation, use the standard engine tool execution path with tools like `browser_open`, `browser_click`, `browser_type`, `browser_extract`, and `browser_screenshot`, or run a session with those tools in the allowlist. The `client.browser` namespace is intentionally limited to diagnostics and install flows.

### `client.coder`

The coder namespace now includes project-scoped GitHub Project intake helpers in addition to run APIs.

```typescript
const binding = await client.coder.getProjectBinding("repo-123");

await client.coder.putProjectBinding("repo-123", {
  github_project_binding: {
    owner: "acme-inc",
    project_number: 7,
    repo_slug: "acme-inc/tandem",
  },
});

const inbox = await client.coder.getProjectGithubInbox("repo-123");

const intake = await client.coder.intakeProjectItem("repo-123", {
  project_item_id: inbox.items[0].project_item_id,
  source_client: "sdk_test",
});
```

Use this flow when you want Tandem to:

- treat GitHub Projects as intake plus visibility
- create Tandem-native coder runs from issue-backed TODO items
- keep Tandem as the execution authority after intake
- inspect schema drift through `schema_drift` / `live_schema_fingerprint`

### `client.skills`

```typescript
const { skills } = await client.skills.list();
const skill = await client.skills.get("security-auditor");
const templates = await client.skills.templates();

await client.skills.import({
  location: "workspace",
  content: yamlString,
  conflict_policy: "overwrite",
});
```

### `client.resources`

```typescript
await client.resources.write({
  key: "agent-config/alert-threshold",
  value: { threshold: 0.95 },
});
const { items } = await client.resources.list({ prefix: "agent-config/" });
await client.resources.delete("agent-config/alert-threshold");
```

### `client.routines`

```typescript
await client.routines.create({
  name: "Daily digest",
  schedule: "0 8 * * *",
  entrypoint: "Summarize today's activity and write to daily-digest.md",
  requires_approval: false,
});

const runs = await client.routines.listRuns({ limit: 10 });
await client.routines.approveRun(runs.runs[0].id as string);
await client.routines.pauseRun(runId);
await client.routines.resumeRun(runId);
```

### Conversational authoring flow

Use `workflowPlans` when the user wants Tandem to shape the DAG through conversation and may need one or more clarification turns.

Use `automationsV2` when the workflow shape is already known and you want deterministic payload construction.

```typescript
const draft = await client.workflowPlans.chatStart({
  prompt: "Build a release checklist automation",
  planSource: "control-panel-composer",
});

const revised = await client.workflowPlans.chatMessage({
  planId: draft.plan.plan_id!,
  message: "Add a Slack notification step at the end.",
});

const applied = await client.workflowPlans.apply({
  planId: revised.plan.plan_id,
  creatorId: "demo-operator",
});

await client.automationsV2.runNow(applied.automation?.automation_id);
```

### `client.automationsV2`

Use V2 for persistent multi-agent DAG flows with per-agent model selection.

Agent-ready pattern (manual run, artifact + MCP handoff):

```typescript
await client.automationsV2.create({
  name: "Daily Marketing Engine",
  status: "active",
  schedule: {
    type: "interval",
    interval_seconds: 86400,
    timezone: "UTC",
    misfire_policy: "run_once",
  },
  agents: [
    {
      agent_id: "research",
      display_name: "Research",
      model_policy: {
        default_model: {
          provider_id: "openrouter",
          model_id: "openai/gpt-4o-mini",
        },
      },
      tool_policy: { allowlist: ["read", "websearch"], denylist: [] },
      mcp_policy: {
        allowed_servers: ["composio"],
        allowed_tools: ["mcp.composio.github_issues_list"],
      },
    },
    {
      agent_id: "writer",
      display_name: "Writer",
      model_policy: {
        default_model: {
          provider_id: "openrouter",
          model_id: "anthropic/claude-3.5-sonnet",
        },
      },
      tool_policy: { allowlist: ["read", "write", "edit"], denylist: [] },
      mcp_policy: { allowed_servers: [], allowed_tools: [] },
    },
  ],
  flow: {
    nodes: [
      {
        node_id: "market-scan",
        agent_id: "research",
        objective: "Find trends and audience signals.",
      },
      {
        node_id: "draft-copy",
        agent_id: "writer",
        objective: "Draft campaign copy and CTA variants.",
        depends_on: ["market-scan"],
      },
    ],
  },
});

const runs = await client.automationsV2.listRuns("automation-v2-id", 20);
await client.automationsV2.pauseRun(runs.runs[0].run_id!);
await client.automationsV2.resumeRun(runs.runs[0].run_id!);
```

For the exact same pattern with immediate run + result checks, use:

```typescript
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

For a complex workflow that reads files first, writes a staged artifact, then performs a final MCP action:

```typescript
const complexAutomation = await client.automationsV2.create({
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
          "Find TODO/FIXME patterns in src/, docs/, and README files. Output the top findings in working notes as JSON.",
      },
      {
        node_id: "build_risk_report",
        agent_id: "analyst",
        depends_on: ["scan_sources"],
        objective:
          "Create docs/todo_digest.md with risk tiers, rationale, and exact file references.",
      },
      {
        node_id: "notify_and_link",
        agent_id: "notifier",
        depends_on: ["build_risk_report"],
        objective:
          "Send a short Slack summary and include docs/todo_digest.md as the handoff path.",
      },
    ],
  },
  creator_id: "demo-operator",
});

const complexRun = await client.automationsV2.runNow(complexAutomation.automation?.automation_id);
const complexStatus = await client.automationsV2.getRun(complexRun?.run_id!);
console.log({
  automationId: complexAutomation.automation?.automation_id,
  runId: complexRun?.run_id,
  status: complexStatus?.run?.status,
});
```

### `client.automations` (Legacy Compatibility Path)

Use this for existing installs that still rely on the older mission + policy automation shape. For new automation work, prefer `client.automationsV2`.

```typescript
await client.automations.create({
  name: "Weekly security scan",
  schedule: "0 9 * * 1",
  mission: {
    objective: "Audit the API surface for vulnerabilities",
    success_criteria: ["Report written to reports/security.md"],
  },
  policy: {
    tool: { external_integrations_allowed: false },
    approval: { requires_approval: true },
  },
});

const run = await client.automations.getRun(runId);
await client.automations.approveRun(runId, "LGTM");
```

### `client.workflowPlans`

Use workflow plans when you want the engine planner to draft an automation, iterate on it in chat, then apply it.

```typescript
const started = await client.workflowPlans.chatStart({
  prompt: "Create a release checklist automation",
  planSource: "chat",
});

const updated = await client.workflowPlans.chatMessage({
  planId: started.plan.plan_id!,
  message: "Add a smoke-test step before rollout.",
});

await client.workflowPlans.apply({
  planId: updated.plan.plan_id,
  creatorId: "operator-1",
});
```

### `client.agentTeams`

```typescript
const templates = await client.agentTeams.listTemplates();
const instances = await client.agentTeams.listInstances({ status: "active" });

const result = await client.agentTeams.spawn({
  missionID: "mission-123",
  role: "builder",
  justification: "Implementing feature X",
});

const { spawnApprovals } = await client.agentTeams.listApprovals();
await client.agentTeams.approveSpawn(spawnApprovals[0].approvalID!);

await client.agentTeams.createTemplate({
  template: {
    templateID: "marketing-writer",
    role: "worker",
    system_prompt: "Write concise conversion-focused copy.",
  },
});
await client.agentTeams.updateTemplate("marketing-writer", {
  system_prompt: "Write concise copy with product-proof points.",
});
await client.agentTeams.deleteTemplate("marketing-writer");
```

### `client.missions`

```typescript
const { mission } = await client.missions.create({
  title: "Q1 Security Hardening",
  goal: "Audit and fix all critical security issues",
  work_items: [{ title: "Audit auth middleware", assigned_agent: "security-auditor" }],
});

const full = await client.missions.get(mission!.id!);
await client.missions.applyEvent(mission!.id!, {
  type: "work_item.completed",
  work_item_id: "...",
});
```

### `client.optimizations`

Use optimizations to create and manage AutoResearch workflow optimization campaigns. Campaigns generate candidate workflow prompts, evaluate them against baseline runs, and apply approved winners back to the live workflow.

```typescript
// List all optimization campaigns
const { optimizations, count } = await client.optimizations.list();

// Create a new optimization campaign
const { optimization } = await client.optimizations.create({
  name: "Improve research quality",
  source_workflow_id: "workflow-abc123",
  artifacts: {
    objective_ref: "objective.yaml",
    eval_ref: "eval.yaml",
    mutation_policy_ref: "mutation_policy.yaml",
    scope_ref: "scope.yaml",
    budget_ref: "budget.yaml",
  },
});

// Get campaign details with experiment count
const details = await client.optimizations.get(optimization.optimization_id!);

// Trigger actions on a campaign (e.g., queue baseline replay, generate candidates)
await client.optimizations.action(optimization.optimization_id!, {
  action: "queue_replay",
  run_id: "run-xyz",
});

// List experiments for a campaign
const { experiments } = await client.optimizations.listExperiments(optimization.optimization_id!);

// Get a specific experiment
const experiment = await client.optimizations.getExperiment(
  optimization.optimization_id!,
  experiments[0].experiment_id!
);

// Apply an approved winner back to the live workflow
const { automation } = await client.optimizations.applyWinner(
  optimization.optimization_id!,
  experiments[0].experiment_id!
);
```

Available campaign actions via `action()`:

- `queue_replay` — Queue a baseline replay run to re-establish metrics
- `generate_candidate` — Generate the next bounded candidate for evaluation
- `approve` / `reject` — Mark an experiment as approved or rejected
- `apply` — Apply an approved winner to the live workflow
