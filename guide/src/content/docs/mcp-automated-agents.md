---
title: MCP Automated Agents
---

Set up scheduled agents that can use MCP connector tools with explicit per-agent tool allowlists.

## What You Get

- MCP connector lifecycle: add, enable/disable, connect, refresh
- Auto MCP tool discovery on connect (`initialize` + `tools/list`)
- Namespaced MCP tools in the global tool registry (for example `mcp.arcade.search`)
- Routine-level `allowed_tools` policy for scheduled bots
- Agent Automation visibility for connector status and scheduled runs

## 1) Configure MCP Connector

Add an MCP server:

```bash
curl -sS -X POST http://127.0.0.1:39731/mcp \
  -H "content-type: application/json" \
  -d '{
    "name": "arcade",
    "transport": "https://your-mcp-server.example/mcp",
    "enabled": true,
    "headers": {
      "Authorization": "Bearer YOUR_TOKEN"
    }
  }'
```

Connect it (this performs discovery and caches tools):

```bash
curl -sS -X POST http://127.0.0.1:39731/mcp/arcade/connect
```

Refresh cached tools later:

```bash
curl -sS -X POST http://127.0.0.1:39731/mcp/arcade/refresh
```

List connector tools:

```bash
curl -sS http://127.0.0.1:39731/mcp/tools
```

List all tool IDs (built-ins + MCP):

```bash
curl -sS http://127.0.0.1:39731/tool/ids
```

### Provider Notes: Arcade

Arcade MCP Gateways are ideal when you want to curate a smaller, safer tool set for a specific bot.

Recommended flow:

1. In Arcade, create an MCP Gateway for your bot mission.
2. Select only the tools needed for that mission (avoid sending dozens of tools to one agent).
3. Use the gateway URL in Tandem as the MCP `transport`.
4. If using Arcade headers auth mode, pass required headers in Tandem (`Authorization`, and user ID header if your setup requires it).

Then in Tandem:

```bash
curl -sS -X POST http://127.0.0.1:39731/mcp \
  -H "content-type: application/json" \
  -d '{
    "name": "arcade",
    "transport": "https://api.arcade.dev/mcp/YOUR-GATEWAY-SLUG",
    "enabled": true,
    "headers": {
      "Authorization": "Bearer YOUR_ARCADE_KEY"
    }
  }'
curl -sS -X POST http://127.0.0.1:39731/mcp/arcade/connect
```

### Provider Notes: Composio

For Composio MCP URLs, make sure your project’s MCP URL security requirements are satisfied.

Important dates from Composio changelog:

- Since **December 15, 2025**, new projects require `x-api-key` on MCP URL requests.
- After **April 15, 2026**, requests without `x-api-key` are rejected.

Then in Tandem:

```bash
curl -sS -X POST http://127.0.0.1:39731/mcp \
  -H "content-type: application/json" \
  -d '{
    "name": "composio",
    "transport": "https://mcp.composio.dev/YOUR_MCP_PATH",
    "enabled": true,
    "headers": {
      "x-api-key": "YOUR_COMPOSIO_API_KEY"
    }
  }'
curl -sS -X POST http://127.0.0.1:39731/mcp/composio/connect
```

## 2) Create a Scheduled Agent With Tool Allowlist

Create a routine that only allows selected tools:

```bash
curl -sS -X POST http://127.0.0.1:39731/routines \
  -H "content-type: application/json" \
  -d '{
    "routine_id": "daily-mcp-research",
    "name": "Daily MCP Research",
    "schedule": { "interval_seconds": { "seconds": 86400 } },
    "entrypoint": "mission.default",
    "allowed_tools": ["mcp.arcade.search", "read"],
    "output_targets": ["file://reports/daily-mcp-research.json"],
    "requires_approval": true,
    "external_integrations_allowed": true
  }'
```

Trigger immediately:

```bash
curl -sS -X POST http://127.0.0.1:39731/routines/daily-mcp-research/run_now \
  -H "content-type: application/json" \
  -d '{}'
```

Check run records:

```bash
curl -sS "http://127.0.0.1:39731/routines/runs?routine_id=daily-mcp-research&limit=10"
```

Each run record includes `allowed_tools` so you can verify tool scope at execution time.

## 2.5) Which Tools Should You Start With?

For autonomous bots, start narrow and expand only when runs are stable.

### Recommended Starter Bundle (Most Teams)

Use one read channel + one write channel + one source-of-truth system:

- `mcp.<provider>.search/list/get` tools for discovery and context
- One ticketing/project write tool (create/update comment/status)
- One messaging output tool (post update to Slack/Teams/Discord)

### Good First Mission Patterns

1. Triage bot:
   - read inbox/issues
   - classify
   - post summary + suggested next steps
2. Status reporter:
   - read project state + CI/test signals
   - publish daily report artifact + message channel post
3. Change assistant (with approval):
   - prepare updates/comments/PR drafts
   - require approval for any external side-effect tools

### Avoid Early

- Huge tool surfaces in one agent (harder model routing, more mistakes)
- Destructive tools without approval gates
- Multi-system write actions in the same first rollout

### Practical Rule

Start with **3-8 allowed tools** max per routine. Increase only after you get predictable run history.

## 2.6) Ready-Made Routine Templates (Copy/Paste)

Replace MCP tool IDs with your connector namespaces (for example `mcp.arcade.search` or `mcp.composio.github_issues_list`).

### Template A: Daily Research Digest

```bash
curl -sS -X POST http://127.0.0.1:39731/routines \
  -H "content-type: application/json" \
  -d '{
    "routine_id": "daily-research-digest",
    "name": "Daily Research Digest",
    "schedule": { "interval_seconds": { "seconds": 86400 } },
    "entrypoint": "mission.default",
    "allowed_tools": [
      "websearch",
      "webfetch_document",
      "read",
      "write",
      "mcp.arcade.search"
    ],
    "output_targets": ["file://reports/daily-research-digest.md"],
    "requires_approval": true,
    "external_integrations_allowed": true
  }'
```

### Template B: 15-Min Issue Triage

```bash
curl -sS -X POST http://127.0.0.1:39731/routines \
  -H "content-type: application/json" \
  -d '{
    "routine_id": "issue-triage-bot",
    "name": "Issue Triage Bot",
    "schedule": { "interval_seconds": { "seconds": 900 } },
    "entrypoint": "mission.default",
    "allowed_tools": [
      "read",
      "write",
      "websearch",
      "webfetch_document",
      "mcp.composio.github_issues_list",
      "mcp.composio.github_issue_comment_create"
    ],
    "output_targets": ["file://reports/issue-triage.json"],
    "requires_approval": true,
    "external_integrations_allowed": true
  }'
```

### Template C: Hourly Release Reporter (Unattended)

```bash
curl -sS -X POST http://127.0.0.1:39731/routines \
  -H "content-type: application/json" \
  -d '{
    "routine_id": "hourly-release-reporter",
    "name": "Hourly Release Reporter",
    "schedule": { "interval_seconds": { "seconds": 3600 } },
    "entrypoint": "mission.default",
    "allowed_tools": [
      "read",
      "websearch",
      "webfetch_document",
      "write",
      "mcp.arcade.search"
    ],
    "output_targets": ["file://reports/release-status.md"],
    "requires_approval": false,
    "external_integrations_allowed": true
  }'
```

## 3) Desktop Flow (Agent Automation)

From desktop:

1. Open `Extensions -> MCP` and add/connect connector servers.
2. Open `Agent Automation` (robot icon in the left nav) and use `Automated Bots`.
3. Create a scheduled bot:
   - choose interval
   - choose entrypoint (for example `mcp.arcade.search`)
   - choose `allowed_tools` from MCP and built-ins
4. Use `Configured Routines` actions to pause/resume routines.
5. Use `Scheduled Bots` run actions (`Approve`, `Deny`, `Pause`, `Resume`) for gated runs.
6. In `Scheduled Bots`, inspect tool scope shown on each run card.

## 4) SSE Visibility

Watch routine stream:

```bash
curl -N http://127.0.0.1:39731/routines/events
```

Relevant events include:

- `mcp.server.connected`
- `mcp.tools.updated`
- `routine.run.created`
- `routine.approval_required`
- `routine.blocked`

## 5) End-to-End Headless Example Scripts

Use the included scripts:

- `examples/headless/mcp_tools_allowlist/flow.sh`
- `examples/headless/mcp_tools_allowlist/flow.ps1`

They automate:

1. MCP add/connect
2. MCP + global tool listing
3. Routine creation with `allowed_tools` + `output_targets`
4. Run trigger + run record/artifact verification

## 6) Headless "Just Run" Setup

Use this when you want unattended operation without opening desktop.

### A) Start engine headless

```bash
cargo run -p tandem-ai --bin tandem-engine -- serve --host 127.0.0.1 --port 39731
```

### B) Register MCP and routine once

Use Sections 1 and 2 above (or run the included script):

- `examples/headless/mcp_tools_allowlist/flow.sh`
- `examples/headless/mcp_tools_allowlist/flow.ps1`

### C) Make it autonomous

For no human intervention:

- set `requires_approval: false` on routines you trust
- keep `allowed_tools` explicit and small
- keep `output_targets` configured so every run leaves artifacts

### D) Observe continuously

```bash
curl -N http://127.0.0.1:39731/routines/events
```

Watch for:

- `routine.run.created`
- `routine.blocked`
- `routine.approval_required`
- `mcp.tools.updated`

### E) Production tip

Run `tandem-engine serve` under a process supervisor (systemd, PM2, container orchestrator, or Windows Task Scheduler/service wrapper) so it auto-restarts after reboots/crashes.

## Safety Notes

- Keep connector secrets in headers/env, not logs.
- Default external side-effects are policy-gated (`requires_approval` and `external_integrations_allowed`).
- Prefer explicit `allowed_tools` for production automated agents.

## See Also

- [Headless Service](./headless-service/)
- [Agent Command Center](./agent-command-center/)
- [WebMCP for Agents](./webmcp-for-agents/)
- [Engine Commands](./reference/engine-commands/)
- [Tools Reference](./reference/tools/)
