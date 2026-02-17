param(
    [string]$HostName = "127.0.0.1",
    [int]$Port = 39731,
    [string]$StateDir = ".tandem-smoke",
    [string]$OutDir = "runtime-proof",
    [int]$HealthTimeoutSeconds = 30
)

$ErrorActionPreference = "Stop"

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$outPath = Join-Path $repoRoot $OutDir
$statePath = Join-Path $repoRoot $StateDir

New-Item -ItemType Directory -Force -Path $outPath, $statePath | Out-Null
Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $outPath "*")

Get-Process | Where-Object { $_.ProcessName -like "tandem-engine*" } | Stop-Process -Force -ErrorAction SilentlyContinue

Push-Location $repoRoot
try {
    cargo build -p tandem-ai | Out-Host

    $stdoutLog = Join-Path $outPath "serve.stdout.log"
    $stderrLog = Join-Path $outPath "serve.stderr.log"
    $engine = Start-Process `
        -FilePath (Join-Path $repoRoot "target\debug\tandem-engine.exe") `
        -ArgumentList @("serve", "--host", $HostName, "--port", "$Port", "--state-dir", $statePath) `
        -WorkingDirectory $repoRoot `
        -RedirectStandardOutput $stdoutLog `
        -RedirectStandardError $stderrLog `
        -PassThru

    $sseJob = $null
    try {
        $healthUri = "http://$HostName`:$Port/global/health"
        $ok = $false
        for ($i = 0; $i -lt ($HealthTimeoutSeconds * 2); $i++) {
            try {
                $health = Invoke-RestMethod -Method GET -Uri $healthUri
                $ok = $true
                break
            } catch {
                Start-Sleep -Milliseconds 500
            }
        }
        if (-not $ok) {
            throw "Engine did not become healthy on $healthUri"
        }

        $health | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $outPath "health.json")

        $session = Invoke-RestMethod -Method POST -Uri "http://$HostName`:$Port/session" -ContentType "application/json" -Body "{}"
        $session | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $outPath "session.create.json")
        $sid = $session.id
        $sid | Set-Content (Join-Path $outPath "session.id.txt")

        Invoke-RestMethod -Method GET -Uri "http://$HostName`:$Port/session" |
            ConvertTo-Json -Depth 10 |
            Set-Content (Join-Path $outPath "session.list.json")

        $msgPayload = @{ parts = @(@{ type = "text"; text = "message for smoke test" }) } | ConvertTo-Json -Depth 10
        Invoke-RestMethod -Method POST -Uri "http://$HostName`:$Port/session/$sid/message" -ContentType "application/json" -Body $msgPayload |
            ConvertTo-Json -Depth 10 |
            Set-Content (Join-Path $outPath "session.post_message.json")

        Invoke-RestMethod -Method GET -Uri "http://$HostName`:$Port/session/$sid/message" |
            ConvertTo-Json -Depth 10 |
            Set-Content (Join-Path $outPath "session.messages.json")

        Invoke-RestMethod -Method GET -Uri "http://$HostName`:$Port/provider" |
            ConvertTo-Json -Depth 10 |
            Set-Content (Join-Path $outPath "provider.list.json")

        $eventLog = Join-Path $outPath "event.log"
        $sseJob = Start-Job -ScriptBlock {
            param($uri, $path)
            & curl.exe -N -s $uri | Tee-Object -FilePath $path | Out-Null
        } -ArgumentList @("http://$HostName`:$Port/event", $eventLog)

        Start-Sleep -Seconds 1
        $asyncPayload = @{ parts = @(@{ type = "text"; text = "hello streaming" }) } | ConvertTo-Json -Depth 10
        Invoke-RestMethod -Method POST -Uri "http://$HostName`:$Port/session/$sid/prompt_async" -ContentType "application/json" -Body $asyncPayload | Out-Null

        $hasEvent = $false
        for ($i = 0; $i -lt 20; $i++) {
            if (Test-Path $eventLog) {
                $line = Select-String -Path $eventLog -Pattern "message.part.updated" -SimpleMatch -ErrorAction SilentlyContinue | Select-Object -First 1
                if ($line) {
                    $line.Line | Set-Content (Join-Path $outPath "sse.message.part.updated.line.txt")
                    $hasEvent = $true
                    break
                }
            }
            Start-Sleep -Milliseconds 500
        }
        if (-not $hasEvent) {
            throw "Did not capture message.part.updated in SSE stream"
        }

        Start-Sleep -Seconds 60
        $idle = Get-Process -Id $engine.Id | Select-Object Id, ProcessName, WS, PM, CPU
        $idle | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $outPath "memory.idle.json")

        $toolPayload = @{ parts = @(@{ type = "text"; text = "/tool todo_write {`"todos`":[{`"content`":`"runtime proof todo`"}]}" }) } | ConvertTo-Json -Depth 10
        Invoke-RestMethod -Method POST -Uri "http://$HostName`:$Port/session/$sid/prompt_async" -ContentType "application/json" -Body $toolPayload | Out-Null

        $permissionId = $null
        for ($i = 0; $i -lt 30; $i++) {
            $permissions = Invoke-RestMethod -Method GET -Uri "http://$HostName`:$Port/permission"
            $pending = $permissions | Where-Object { $_.status -eq "pending" } | Select-Object -First 1
            if ($pending) {
                $permissionId = $pending.id
                break
            }
            Start-Sleep -Milliseconds 300
        }
        if ($permissionId) {
            $reply = @{ reply = "allow" } | ConvertTo-Json
            Invoke-RestMethod -Method POST -Uri "http://$HostName`:$Port/permission/$permissionId/reply" -ContentType "application/json" -Body $reply |
                ConvertTo-Json -Depth 10 |
                Set-Content (Join-Path $outPath "permission.reply.json")
        }

        $samples = @()
        1..15 | ForEach-Object {
            $samples += (Get-Process -Id $engine.Id | Select-Object WS, PM, CPU)
            Start-Sleep -Seconds 2
        }
        $samples | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $outPath "memory.samples.json")
        $peak = $samples | Sort-Object WS -Descending | Select-Object -First 1
        $peak | ConvertTo-Json -Depth 10 | Set-Content (Join-Path $outPath "memory.peak.json")

        Write-Host "Smoke test PASS. Artifacts in $outPath"
    } finally {
        if ($sseJob) {
            Stop-Job $sseJob -ErrorAction SilentlyContinue
            Receive-Job $sseJob -ErrorAction SilentlyContinue | Out-Null
            Remove-Job $sseJob -Force -ErrorAction SilentlyContinue
        }
        Get-Process -Id $engine.Id -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }
} finally {
    Get-Process | Where-Object { $_.ProcessName -like "tandem-engine*" } | Stop-Process -Force -ErrorAction SilentlyContinue
    Pop-Location
}
