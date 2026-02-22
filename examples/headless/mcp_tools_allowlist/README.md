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

## Mission/Automation Cold Start Benchmark

If you want to answer "how long does it take to launch the engine and start a mission/automation run?", use the benchmark scripts in this folder.

What is measured per trial:

1. `engine_boot_ms`: engine process launch -> `/global/health` reports `ready=true`
2. `run_now_ack_ms`: `POST /routines/{id}/run_now` (or `/automations/{id}/run_now`) request latency
3. `run_visible_ms`: `run_now` sent -> `GET /routines/runs/{run_id}` (or `/automations/runs/{run_id}`) first returns 200
4. `cold_start_to_run_visible_ms`: `engine_boot_ms + run_visible_ms`

### PowerShell

```powershell
cd examples/headless/mcp_tools_allowlist
$env:BENCH_RUNS = "10"
.\benchmark_cold_start.ps1
```

Optional env:

- `TANDEM_AUTOMATION_API=routines|automations` (default: `routines`)
- `TANDEM_BASE_URL` (default: `http://127.0.0.1:39731`)
- `TANDEM_ENGINE_CMD` (override engine launch command)

### Bash

```bash
cd examples/headless/mcp_tools_allowlist
chmod +x benchmark_cold_start.sh
BENCH_RUNS=10 ./benchmark_cold_start.sh
```

Optional env:

- `TANDEM_AUTOMATION_API=routines|automations` (default: `routines`)
- `TANDEM_BASE_URL` (default: `http://127.0.0.1:39731`)
- `TANDEM_ENGINE_CMD` (override engine launch command)

Both scripts write raw trial data to `cold_start_results.json` in this folder so you can post exact p50/p95 numbers publicly.
