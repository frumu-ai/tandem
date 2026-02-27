param(
    [switch]$DryRun,
    [switch]$Provenance,
    [string]$Otp
)

$ErrorActionPreference = "Stop"

function Invoke-NpmText {
    param(
        [Parameter(Mandatory = $true)][string]$Command,
        [string]$WorkingDirectory = $PWD.Path
    )

    Push-Location $WorkingDirectory
    try {
        # Route through cmd so stderr is redirected into stdout text and won't
        # surface as PowerShell NativeCommandError records.
        $output = cmd /d /c "$Command 2>&1" | Out-String
        $exitCode = $LASTEXITCODE
        return [PSCustomObject]@{
            ExitCode = $exitCode
            Output   = $output
        }
    } finally {
        Pop-Location
    }
}

$logFile = if ($env:PUBLISH_NPM_LOG) { $env:PUBLISH_NPM_LOG } else { "publish-npm.log" }
if (Test-Path $logFile) { Remove-Item $logFile -Force }
New-Item -ItemType File -Path $logFile -Force | Out-Null

$packages = @(
    "packages/tandem-engine",
    "packages/tandem-tui",
    "packages/tandem-client-ts"
)

Write-Host "Publishing npm wrappers..."
if ($DryRun) {
    Write-Host "Mode: dry-run"
}

if (-not $DryRun -and $Otp) {
    if ($Otp -notmatch '^\d{6,8}$') {
        throw "Invalid -Otp value. Use a numeric authenticator code (usually 6 digits), not an npm access token."
    }
}

foreach ($dir in $packages) {
    if (-not (Test-Path $dir)) {
        "SKIP $dir (missing directory)" | Tee-Object -FilePath $logFile -Append
        continue
    }

    $pkgJsonPath = Join-Path $dir "package.json"
    $pkg = Get-Content $pkgJsonPath | ConvertFrom-Json
    $name = $pkg.name
    $version = $pkg.version

    "Processing $name@$version ($dir)" | Tee-Object -FilePath $logFile -Append

    $view = Invoke-NpmText -Command "npm view $name@$version version"
    $viewOutput = $view.Output
    if ($view.ExitCode -eq 0) {
        "SKIP $name@$version already published" | Tee-Object -FilePath $logFile -Append
        continue
    }
    # First publish often returns E404 (package/version not found). That is expected.
    # Do not fail on auth-ish notices when E404 is also present.
    if (($viewOutput -match "E404|Not Found|is not in this registry") -eq $false -and
        $viewOutput -match "Access token expired or revoked|E401|ENEEDAUTH|Unable to authenticate") {
        throw @"
npm authentication failed while checking $name@$version.
Run:
  npm logout
  npm login
Then retry:
  .\scripts\publish-npm-ci.ps1 -DryRun
"@
    }

    $publishCommand = "npm publish --access public"
    if ($Provenance) {
        $publishCommand += " --provenance"
    }

    # TS SDK publish path: build explicitly, then publish without lifecycle scripts.
    # This avoids npm workspace dependency resolution failures in CI.
    if ($dir -eq "packages/tandem-client-ts") {
        "Building JS bundles for $name@$version with npx tsup" | Tee-Object -FilePath $logFile -Append
        $buildJs = Invoke-NpmText -WorkingDirectory $dir -Command "npx --yes -p tsup -p typescript -p zod tsup src/index.ts --format esm,cjs --clean"
        $buildJs.Output | Tee-Object -FilePath $logFile -Append | Out-Null
        if ($buildJs.ExitCode -ne 0) {
            throw "Failed JS bundle build for $name@$version"
        }

        "Building type declarations for $name@$version with npx tsc" | Tee-Object -FilePath $logFile -Append
        $buildDts = Invoke-NpmText -WorkingDirectory $dir -Command "npx --yes -p typescript tsc --project tsconfig.json --emitDeclarationOnly"
        $buildDts.Output | Tee-Object -FilePath $logFile -Append | Out-Null
        if ($buildDts.ExitCode -ne 0) {
            throw "Failed declaration build for $name@$version"
        }
        $publishCommand += " --ignore-scripts"
    }

    if ($DryRun) {
        $publish = Invoke-NpmText -WorkingDirectory $dir -Command "$publishCommand --dry-run"
    } else {
        if ($Otp) {
            $publishCommand += " --otp $Otp"
        }
        $publish = Invoke-NpmText -WorkingDirectory $dir -Command $publishCommand
    }

    $publish.Output | Tee-Object -FilePath $logFile -Append | Out-Null

    if ($publish.ExitCode -ne 0) {
        if ($publish.Output -match "EOTP|one-time password|--otp=<code>") {
            throw @"
npm publish requires an OTP code for this account.
Retry with:
  .\scripts\publish-npm-ci.ps1 -Otp <6-digit-code>

If your code expired, generate a fresh one and retry immediately.
"@
        }
        if ($publish.Output -match "Access token expired or revoked|E401|ENEEDAUTH|Unable to authenticate") {
            throw @"
npm authentication failed while publishing $name@$version.
Run:
  npm logout
  npm login
Then retry:
  .\scripts\publish-npm-ci.ps1 -DryRun
"@
        }
        throw "Failed publishing $name@$version"
    }

    "OK $name@$version" | Tee-Object -FilePath $logFile -Append
}

"npm publish flow completed." | Tee-Object -FilePath $logFile -Append
