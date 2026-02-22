# MCP Tool Allowlist (Headless)

This example demonstrates a full headless flow for MCP connector tools and routine tool allowlists.

## What it covers

1. Add an MCP server
2. Connect and auto-discover MCP tools (`initialize` + `tools/list`)
3. List MCP tools and global tool registry entries
4. Create a routine with an explicit `allowed_tools` subset
5. Configure routine `output_targets` for artifact destination wiring
6. Trigger a run and verify the run record includes the same `allowed_tools`
7. Watch SSE for MCP and routine events

## Prerequisites

- Tandem engine running with HTTP API (default: `http://127.0.0.1:39731`)
- Optional: `jq` for shell script output formatting

## Environment

Set these before running:

- `TANDEM_BASE_URL` (optional, defaults to `http://127.0.0.1:39731`)
- `MCP_SERVER_NAME` (optional, defaults to `arcade`)
- `MCP_TRANSPORT` (required, example: `https://mcp.arcade.dev/mcp`)
- `MCP_AUTH_BEARER` (optional bearer token; sent as `Authorization: Bearer ...`)
- `TANDEM_AUTOMATION_API` (optional: `routines` or `automations`, defaults to `routines`)

## Run (Bash)

```bash
cd examples/headless/mcp_tools_allowlist
chmod +x flow.sh
MCP_TRANSPORT="https://your-mcp-server.example/mcp" ./flow.sh
```

## Run (PowerShell)

```powershell
cd examples/headless/mcp_tools_allowlist
$env:MCP_TRANSPORT = "https://your-mcp-server.example/mcp"
./flow.ps1
```

## SSE watch (separate terminal)

```bash
curl -N "$TANDEM_BASE_URL/routines/events"
```

You should see events including:

- `mcp.server.connected`
- `mcp.tools.updated`
- `routine.run.created`
