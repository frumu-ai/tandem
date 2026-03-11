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
await client.channels.put("telegram", {
  token: "bot:xxx",
  allowed_users: ["@yourhandle"],
});
const status = await client.channels.status();
console.log(status.telegram.connected);
```

### `client.mcp`

```typescript
await client.mcp.add({ name: "arcade", transport: "https://mcp.arcade.ai/mcp" });
await client.mcp.connect("arcade");
const tools = await client.mcp.listTools();
const resources = await client.mcp.listResources();
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

### `client.automations`

Automations differ from routines: they use a **mission + policy** model with multi-agent orchestration.

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

### `client.automationsV2`

Use V2 for persistent multi-agent DAG flows with per-agent model selection.

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
      mcp_policy: { allowed_servers: ["composio"] },
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
      mcp_policy: { allowed_servers: [] },
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
