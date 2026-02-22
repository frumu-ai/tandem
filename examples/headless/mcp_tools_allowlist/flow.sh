#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${TANDEM_BASE_URL:-http://127.0.0.1:39731}"
SERVER_NAME="${MCP_SERVER_NAME:-arcade}"
TRANSPORT="${MCP_TRANSPORT:-}"
API_FAMILY="${TANDEM_AUTOMATION_API:-routines}"
ROUTINE_ID="routine-mcp-allowlist-$(date +%s)"

if [[ -z "$TRANSPORT" ]]; then
  echo "MCP_TRANSPORT is required (example: https://your-mcp-server.example/mcp)"
  exit 1
fi

if [[ -n "${MCP_AUTH_BEARER:-}" ]]; then
  HEADERS_JSON="{\"Authorization\":\"Bearer ${MCP_AUTH_BEARER}\"}"
else
  HEADERS_JSON="{}"
fi

echo "== Add MCP server =="
curl -sS -X POST "$BASE_URL/mcp" \
  -H "content-type: application/json" \
  -d "{\"name\":\"$SERVER_NAME\",\"transport\":\"$TRANSPORT\",\"enabled\":true,\"headers\":$HEADERS_JSON}"
echo

echo "== Connect MCP server (auto tools discovery) =="
curl -sS -X POST "$BASE_URL/mcp/$SERVER_NAME/connect"
echo

echo "== List MCP tools =="
curl -sS "$BASE_URL/mcp/tools"
echo

echo "== List global tool IDs (look for mcp.$SERVER_NAME.*) =="
curl -sS "$BASE_URL/tool/ids"
echo

TOOL_ONE="mcp.${SERVER_NAME}.search"
TOOL_TWO="read"

if [[ "$API_FAMILY" == "automations" ]]; then
  CREATE_PATH="/automations"
  RUN_NOW_PATH="/automations/$ROUTINE_ID/run_now"
  RUN_PATH_PREFIX="/automations/runs"
  RESOURCE_LABEL="Automation"
else
  CREATE_PATH="/routines"
  RUN_NOW_PATH="/routines/$ROUTINE_ID/run_now"
  RUN_PATH_PREFIX="/routines/runs"
  RESOURCE_LABEL="Routine"
fi

echo "== Create routine with allowlist =="
curl -sS -X POST "$BASE_URL$CREATE_PATH" \
  -H "content-type: application/json" \
  -d "{
    \"routine_id\": \"$ROUTINE_ID\",
    \"name\": \"MCP Allowlist Routine\",
    \"schedule\": { \"interval_seconds\": { \"seconds\": 300 } },
    \"entrypoint\": \"mission.default\",
    \"allowed_tools\": [\"$TOOL_ONE\", \"$TOOL_TWO\"],
    \"output_targets\": [\"file://reports/$ROUTINE_ID.json\"],
    \"requires_approval\": true,
    \"external_integrations_allowed\": true
  }"
echo

echo "== Trigger routine run =="
RUN_NOW="$(curl -sS -X POST "$BASE_URL$RUN_NOW_PATH" -H "content-type: application/json" -d "{}")"
echo "$RUN_NOW"
echo

if command -v jq >/dev/null 2>&1; then
  RUN_ID="$(echo "$RUN_NOW" | jq -r '.runID // .runId // .run_id // .id // .run.runID // .run.runId // .run.run_id // .run.id')"
else
  RUN_ID="$(echo "$RUN_NOW" | sed -n 's/.*"runID":"\([^"]*\)".*/\1/p')"
fi

if [[ -z "$RUN_ID" || "$RUN_ID" == "null" ]]; then
  echo "Could not parse run ID from response"
  exit 1
fi

echo "== Fetch run record and verify allowed_tools =="
curl -sS "$BASE_URL$RUN_PATH_PREFIX/$RUN_ID"
echo

echo "== Done =="
echo "$RESOURCE_LABEL: $ROUTINE_ID"
echo "Run:     $RUN_ID"
