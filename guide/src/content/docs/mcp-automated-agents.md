---
title: MCP Automated Agents
---

Set up scheduled agents that can use MCP connector tools with explicit per-agent tool allowlists.

## What You Get

- MCP connector lifecycle: add, enable/disable, connect, refresh
- Auto MCP tool discovery on connect (`initialize` + `tools/list`)
- Namespaced MCP tools in the global tool registry (for example `mcp.arcade.search`)
- Routine-level `allowed_tools` policy for scheduled bots
- Command Center visibility for connector status and scheduled runs

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

## 3) Desktop Flow (Command Center)

From desktop:

1. Open `Extensions -> MCP` and add/connect connector servers.
2. Open `Command Center -> Automation Wiring`.
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
