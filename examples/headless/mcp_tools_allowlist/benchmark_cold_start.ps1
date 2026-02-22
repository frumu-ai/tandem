$ErrorActionPreference = "Stop"

# Measures process-cold automation startup by restarting engine each trial and timing:
# boot readiness, run_now ACK, and run record visibility.

$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$repoRoot = Resolve-Path (Join-Path $scriptDir "..\..\..")

$baseUrl = if ($env:TANDEM_BASE_URL) { $env:TANDEM_BASE_URL } else { "http://127.0.0.1:39731" }
$apiFamily = if ($env:TANDEM_AUTOMATION_API) { $env:TANDEM_AUTOMATION_API } else { "routines" }
$runs = if ($env:BENCH_RUNS) { [int]$env:BENCH_RUNS } else { 5 }
$startupTimeoutSeconds = if ($env:BENCH_STARTUP_TIMEOUT_SECONDS) { [int]$env:BENCH_STARTUP_TIMEOUT_SECONDS } else { 45 }
$runVisibleTimeoutSeconds = if ($env:BENCH_RUN_VISIBLE_TIMEOUT_SECONDS) { [int]$env:BENCH_RUN_VISIBLE_TIMEOUT_SECONDS } else { 20 }
$pollMs = if ($env:BENCH_POLL_MS) { [int]$env:BENCH_POLL_MS } else { 200 }

$host = "127.0.0.1"
$port = "39731"
if ($baseUrl -match "^https?://([^:/]+):(\d+)") {
  $host = $Matches[1]
  $port = $Matches[2]
}

$engineCmd = if ($env:TANDEM_ENGINE_CMD) {
  $env:TANDEM_ENGINE_CMD
} else {
  "`"$repoRoot\target\debug\tandem-engine.exe`" serve --host $host --port $port"
}

if ($apiFamily -eq "automations") {
  $createPath = "/automations"
  $runNowPathTemplate = "/automations/{0}/run_now"
  $runPathPrefix = "/automations/runs"
} else {
  $createPath = "/routines"
  $runNowPathTemplate = "/routines/{0}/run_now"
  $runPathPrefix = "/routines/runs"
}

function Wait-HealthReady {
  param(
    [string]$Url,
    [int]$TimeoutSeconds,
    [int]$PollMs
  )

  $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
  while ((Get-Date) -lt $deadline) {
    try {
      $health = Invoke-RestMethod -Method Get -Uri "$Url/global/health" -TimeoutSec 3
      if ($health.ready -eq $true) {
        return $true
      }
    } catch {
      # keep polling
    }
    Start-Sleep -Milliseconds $PollMs
  }
  return $false
}

function Parse-RunId {
  param([object]$RunNowResponse)
  $runId = $RunNowResponse.runID
  if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $RunNowResponse.runId }
  if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $RunNowResponse.run_id }
  if ([string]::IsNullOrWhiteSpace($runId) -and $RunNowResponse.run) {
    $runId = $RunNowResponse.run.runID
    if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $RunNowResponse.run.runId }
    if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $RunNowResponse.run.run_id }
    if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $RunNowResponse.run.id }
  }
  if ([string]::IsNullOrWhiteSpace($runId)) { $runId = $RunNowResponse.id }
  return $runId
}

function Get-Percentile {
  param(
    [double[]]$Data,
    [double]$P
  )
  if (-not $Data -or $Data.Count -eq 0) { return $null }
  $sorted = $Data | Sort-Object
  $idx = [Math]::Ceiling($sorted.Count * $P) - 1
  if ($idx -lt 0) { $idx = 0 }
  if ($idx -ge $sorted.Count) { $idx = $sorted.Count - 1 }
  return [double]$sorted[$idx]
}

$results = @()
$benchRoutineId = "bench-cold-start-$([DateTimeOffset]::UtcNow.ToUnixTimeSeconds())"
$createBody = @{
  routine_id = $benchRoutineId
  name = "Cold Start Benchmark Routine"
  schedule = @{
    interval_seconds = @{
      seconds = 3600
    }
  }
  entrypoint = "mission.default"
  allowed_tools = @("read")
  requires_approval = $false
  external_integrations_allowed = $false
}

Write-Host "== Ensure benchmark routine exists =="
Invoke-RestMethod -Method Post -Uri "$baseUrl$createPath" -ContentType "application/json" -Body ($createBody | ConvertTo-Json -Depth 8) | Out-Null

for ($i = 1; $i -le $runs; $i++) {
  Write-Host ""
  Write-Host "== Trial $i/$runs =="

  $engineStartTs = [System.Diagnostics.Stopwatch]::StartNew()
  $proc = Start-Process -FilePath "powershell" -ArgumentList @("-NoProfile", "-Command", $engineCmd) -PassThru -WindowStyle Hidden

  try {
    $ready = Wait-HealthReady -Url $baseUrl -TimeoutSeconds $startupTimeoutSeconds -PollMs $pollMs
    if (-not $ready) {
      throw "Engine did not become ready within $startupTimeoutSeconds seconds"
    }
    $engineBootMs = [double]$engineStartTs.Elapsed.TotalMilliseconds

    $runNowPath = [string]::Format($runNowPathTemplate, $benchRoutineId)
    # API-side enqueue/ack latency for mission-trigger path.
    $runNowTimer = [System.Diagnostics.Stopwatch]::StartNew()
    $runNow = Invoke-RestMethod -Method Post -Uri "$baseUrl$runNowPath" -ContentType "application/json" -Body "{}"
    $runNowAckMs = [double]$runNowTimer.Elapsed.TotalMilliseconds
    $runId = Parse-RunId -RunNowResponse $runNow
    if ([string]::IsNullOrWhiteSpace($runId)) {
      throw "Could not parse run ID from run_now response"
    }

    # End-to-end mission trigger visibility gate (run record exists).
    $visibleTimer = [System.Diagnostics.Stopwatch]::StartNew()
    $visible = $false
    $visibleDeadline = (Get-Date).AddSeconds($runVisibleTimeoutSeconds)
    while ((Get-Date) -lt $visibleDeadline) {
      try {
        Invoke-RestMethod -Method Get -Uri "$baseUrl$runPathPrefix/$runId" -TimeoutSec 3 | Out-Null
        $visible = $true
        break
      } catch {
        Start-Sleep -Milliseconds $pollMs
      }
    }
    if (-not $visible) {
      throw "Run record not visible within $runVisibleTimeoutSeconds seconds"
    }
    $runVisibleMs = [double]$visibleTimer.Elapsed.TotalMilliseconds
    $totalMs = $engineBootMs + $runVisibleMs

    $results += [pscustomobject]@{
      trial = $i
      engine_boot_ms = [Math]::Round($engineBootMs, 2)
      run_now_ack_ms = [Math]::Round($runNowAckMs, 2)
      run_visible_ms = [Math]::Round($runVisibleMs, 2)
      cold_start_to_run_visible_ms = [Math]::Round($totalMs, 2)
      run_id = $runId
    }

    Write-Host ("engine_boot_ms={0:N2} run_now_ack_ms={1:N2} run_visible_ms={2:N2} total_ms={3:N2}" -f $engineBootMs, $runNowAckMs, $runVisibleMs, $totalMs)
  } finally {
    if ($proc -and -not $proc.HasExited) {
      Stop-Process -Id $proc.Id -Force
    }
  }
}

$bootValues = @($results | ForEach-Object { [double]$_.engine_boot_ms })
$ackValues = @($results | ForEach-Object { [double]$_.run_now_ack_ms })
$visibleValues = @($results | ForEach-Object { [double]$_.run_visible_ms })
$totalValues = @($results | ForEach-Object { [double]$_.cold_start_to_run_visible_ms })

Write-Host ""
Write-Host "== Summary =="
Write-Host ("engine_boot_ms     p50={0:N2} p95={1:N2}" -f (Get-Percentile -Data $bootValues -P 0.5), (Get-Percentile -Data $bootValues -P 0.95))
Write-Host ("run_now_ack_ms     p50={0:N2} p95={1:N2}" -f (Get-Percentile -Data $ackValues -P 0.5), (Get-Percentile -Data $ackValues -P 0.95))
Write-Host ("run_visible_ms     p50={0:N2} p95={1:N2}" -f (Get-Percentile -Data $visibleValues -P 0.5), (Get-Percentile -Data $visibleValues -P 0.95))
Write-Host ("cold_start_total   p50={0:N2} p95={1:N2}" -f (Get-Percentile -Data $totalValues -P 0.5), (Get-Percentile -Data $totalValues -P 0.95))

$outPath = Join-Path $scriptDir "cold_start_results.json"
@{
  timestamp = [DateTimeOffset]::UtcNow.ToString("o")
  base_url = $baseUrl
  api_family = $apiFamily
  runs = $runs
  results = $results
} | ConvertTo-Json -Depth 10 | Set-Content -Path $outPath -Encoding UTF8

Write-Host ""
Write-Host "Saved: $outPath"
