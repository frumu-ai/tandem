@echo off
setlocal

if "%~1"=="" (
  echo Usage: %~nx0 ^<urls_file^> [concurrency] [engine_bin]
  exit /b 1
)

set "URLS_FILE=%~1"
set "CONCURRENCY=%~2"
set "ENGINE_BIN=%~3"

if "%CONCURRENCY%"=="" set "CONCURRENCY=8"

if not exist "%URLS_FILE%" (
  echo URLs file not found: %URLS_FILE%
  exit /b 1
)

if "%ENGINE_BIN%"=="" (
  if exist ".\target\debug\tandem-engine.exe" set "ENGINE_BIN=.\target\debug\tandem-engine.exe"
  if exist ".\src-tauri\binaries\tandem-engine.exe" set "ENGINE_BIN=.\src-tauri\binaries\tandem-engine.exe"
)

if "%ENGINE_BIN%"=="" (
  echo Engine binary not found. Pass it as the third argument.
  exit /b 1
)

rem Create a temporary PowerShell script
set "PS_SCRIPT=%TEMP%\bench_%RANDOM%.ps1"

(
  echo $urlsFile = '%URLS_FILE%'
  echo $concurrency = [int]%CONCURRENCY%
  echo $engineBin = '%ENGINE_BIN%'
  echo if ^(-not ^(Test-Path $engineBin^)^) { Write-Error ^('Engine binary not found: ' + $engineBin^); exit 1 }
  echo $urls = Get-Content -LiteralPath $urlsFile ^| ForEach-Object { $_.Trim^(^) } ^| Where-Object { $_ -ne '' }
  echo if ^($urls.Count -eq 0^) { Write-Error 'No URLs found'; exit 1 }
  echo $pool = [runspacefactory]::CreateRunspacePool^(1, $concurrency^); $pool.Open^(^)
  echo $jobs = @^(^)
  echo Write-Host "Starting benchmark with $concurrency concurrent workers..."
  echo Write-Host "Total URLs: $($urls.Count)"
  echo foreach ^($url in $urls^) {
  echo   $ps = [powershell]::Create^(^); $ps.RunspacePool = $pool
  echo   $script = { param^($u, $bin^)
  echo     try {
  echo       $payload = @{ tool = 'webfetch_document'; args = @{ url = $u; return = 'text' } } ^| ConvertTo-Json -Compress
  echo       $sw = [System.Diagnostics.Stopwatch]::StartNew^(^)
  echo       $psi = New-Object System.Diagnostics.ProcessStartInfo
  echo       $psi.FileName = $bin
  echo       $psi.Arguments = 'tool --json -'
  echo       $psi.RedirectStandardInput = $true
  echo       $psi.RedirectStandardOutput = $true
  echo       $psi.RedirectStandardError = $true
  echo       $psi.UseShellExecute = $false
  echo       $p = New-Object System.Diagnostics.Process; $p.StartInfo = $psi
  echo       
  echo       $null = $p.Start^(^)
  echo       
  echo       $nullStream = [System.IO.Stream]::Null
  echo       $copyTask = $p.StandardOutput.BaseStream.CopyToAsync^($nullStream^)
  echo       $errCopyTask = $p.StandardError.BaseStream.CopyToAsync^($nullStream^)
  echo       
  echo       $p.StandardInput.Write^($payload^); $p.StandardInput.Close^(^)
  echo       
  echo       $maxRss = 0
  echo       while ^(-not $p.HasExited^) {
  echo         try {
  echo           $p.Refresh^(^)
  echo           $currentRss = $p.WorkingSet64
  echo           if ^($currentRss -gt $maxRss^) { $maxRss = $currentRss }
  echo         } catch {}
  echo         Start-Sleep -Milliseconds 10
  echo       }
  echo       $p.WaitForExit^(^)
  echo       $sw.Stop^(^)
  echo       
  echo       $rssKb = if ^($maxRss -gt 0^) { [math]::Round^($maxRss / 1024^) } else { -1 }
  echo       [pscustomobject]@{ Url = $u; Elapsed = $sw.Elapsed.TotalSeconds; RssKb = $rssKb }
  echo     } catch {
  echo       Write-Error $_
  echo       return [pscustomobject]@{ Url = $u; Elapsed = -1; RssKb = -1; Error = $_.ToString^(^) }
  echo     }
  echo   }
  echo   $handle = $ps.AddScript^($script^).AddArgument^($url^).AddArgument^($engineBin^).BeginInvoke^(^)
  echo   $jobs += [pscustomobject]@{ PowerShell = $ps; Handle = $handle }
  echo }
  echo 
  echo $completed = 0
  echo $total = $jobs.Count
  echo while ^($completed -lt $total^) {
  echo   $completed = 0
  echo   foreach ^($job in $jobs^) {
  echo     if ^($job.Handle.IsCompleted^) { $completed++ }
  echo   }
  echo   Write-Progress -Activity "Benchmarking" -Status "$completed / $total completed" -PercentComplete ^($completed / $total * 100^)
  echo   Start-Sleep -Milliseconds 200
  echo }
  echo 
  echo $results = foreach ^($job in $jobs^) {
  echo   try {
  echo     $res = $job.PowerShell.EndInvoke^($job.Handle^)
  echo     $job.PowerShell.Dispose^(^)
  echo     $res
  echo   } catch {
  echo     Write-Error "Job failed: $_"
  echo   }
  echo }
  echo $pool.Close^(^)
  echo $results = $results ^| Where-Object { $_ -is [System.Management.Automation.PSCustomObject] }
  echo $outPath = [System.IO.Path]::ChangeExtension^([System.IO.Path]::GetTempFileName^(^), 'tsv'^)
  echo $results ^| ForEach-Object { '{0}`t{1}`t{2}' -f $_.Url, $_.Elapsed, $_.RssKb } ^| Set-Content -LiteralPath $outPath
  echo function Get-Percentile^([double[]] $values, [double] $pct^) {
  echo   if ^($values.Count -eq 0^) { return $null }
  echo   $sorted = $values ^| Sort-Object
  echo   $k = ^($sorted.Count - 1^) * $pct
  echo   $f = [math]::Floor^($k^); $c = [math]::Ceiling^($k^)
  echo   if ^($f -eq $c^) { return $sorted[[int]$k] }
  echo   return $sorted[$f] + ^($sorted[$c] - $sorted[$f]^) * ^($k - $f^)
  echo }
  echo $elapsed = $results ^| ForEach-Object { [double] $_.Elapsed }
  echo $rss = $results ^| Where-Object { $_.RssKb -ge 0 } ^| ForEach-Object { [double] $_.RssKb }
  echo Write-Output ^('runs=' + $results.Count^)
  echo Write-Output ^('p50_elapsed_s=' + ^('{0:N3}' -f ^(Get-Percentile $elapsed 0.5^)^)^)
  echo Write-Output ^('p95_elapsed_s=' + ^('{0:N3}' -f ^(Get-Percentile $elapsed 0.95^)^)^)
  echo if ^($rss.Count -gt 0^) {
  echo   Write-Output ^('p50_rss_kb=' + [int]^(Get-Percentile $rss 0.5^)^)
  echo   Write-Output ^('p95_rss_kb=' + [int]^(Get-Percentile $rss 0.95^)^)
  echo } else {
  echo   Write-Output 'p50_rss_kb=unknown'
  echo   Write-Output 'p95_rss_kb=unknown'
  echo }
  echo Write-Output ^('results_file=' + $outPath^)
) > "%PS_SCRIPT%"

powershell -NoProfile -ExecutionPolicy Bypass -File "%PS_SCRIPT%"
del "%PS_SCRIPT%"
