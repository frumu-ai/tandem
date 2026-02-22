$ErrorActionPreference = "Stop"

$baseUrl = if ($env:TANDEM_BASE_URL) { $env:TANDEM_BASE_URL } else { "http://127.0.0.1:39731" }
$serverName = if ($env:MCP_SERVER_NAME) { $env:MCP_SERVER_NAME } else { "arcade" }
$transport = $env:MCP_TRANSPORT
$apiFamily = if ($env:TANDEM_AUTOMATION_API) { $env:TANDEM_AUTOMATION_API } else { "routines" }
$routineId = "routine-mcp-allowlist-$([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())"

if ([string]::IsNullOrWhiteSpace($transport)) {
  throw "MCP_TRANSPORT is required (example: https://your-mcp-server.example/mcp)"
}

$headersMap = @{}
if (-not [string]::IsNullOrWhiteSpace($env:MCP_AUTH_BEARER)) {
  $headersMap["Authorization"] = "Bearer $($env:MCP_AUTH_BEARER)"
}

Write-Host "== Add MCP server =="
$addBody = @{
  name = $serverName
  transport = $transport
  enabled = $true
  headers = $headersMap
}
Invoke-RestMethod -Method Post -Uri "$baseUrl/mcp" -ContentType "application/json" -Body ($addBody | ConvertTo-Json -Depth 8) | ConvertTo-Json -Depth 8

Write-Host "== Connect MCP server (auto tools discovery) =="
Invoke-RestMethod -Method Post -Uri "$baseUrl/mcp/$serverName/connect" | ConvertTo-Json -Depth 8

Write-Host "== List MCP tools =="
Invoke-RestMethod -Method Get -Uri "$baseUrl/mcp/tools" | ConvertTo-Json -Depth 8

Write-Host "== List global tool IDs (look for mcp.$serverName.*) =="
Invoke-RestMethod -Method Get -Uri "$baseUrl/tool/ids" | ConvertTo-Json -Depth 8

$toolOne = "mcp.$serverName.search"
$toolTwo = "read"

if ($apiFamily -eq "automations") {
  $createPath = "/automations"
  $runNowPath = "/automations/$routineId/run_now"
  $runPathPrefix = "/automations/runs"
  $resourceLabel = "Automation"
} else {
  $createPath = "/routines"
  $runNowPath = "/routines/$routineId/run_now"
  $runPathPrefix = "/routines/runs"
  $resourceLabel = "Routine"
}

Write-Host "== Create routine with allowlist =="
$routineBody = @{
  routine_id = $routineId
  name = "MCP Allowlist Routine"
  schedule = @{
    interval_seconds = @{
      seconds = 300
    }
  }
  entrypoint = "mission.default"
  allowed_tools = @($toolOne, $toolTwo)
  output_targets = @("file://reports/$routineId.json")
  requires_approval = $true
  external_integrations_allowed = $true
}
Invoke-RestMethod -Method Post -Uri "$baseUrl$createPath" -ContentType "application/json" -Body ($routineBody | ConvertTo-Json -Depth 12) | ConvertTo-Json -Depth 12

Write-Host "== Trigger routine run =="
$runNow = Invoke-RestMethod -Method Post -Uri "$baseUrl$runNowPath" -ContentType "application/json" -Body "{}"
$runNow | ConvertTo-Json -Depth 12

$runId = $runNow.runID
if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $runNow.runId }
if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $runNow.run_id }
if ([string]::IsNullOrWhiteSpace($runId) -and $runNow.run) {
  $runId = $runNow.run.runID
  if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $runNow.run.runId }
  if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $runNow.run.run_id }
  if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $runNow.run.id }
}
if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $runNow.id }
if ([string]::IsNullOrWhiteSpace($runId)) {
  throw "Could not parse run ID from response"
}

Write-Host "== Fetch run record and verify allowed_tools =="
Invoke-RestMethod -Method Get -Uri "$baseUrl$runPathPrefix/$runId" | ConvertTo-Json -Depth 12

Write-Host "== Done =="
Write-Host "$resourceLabel: $routineId"
Write-Host "Run:     $runId"
