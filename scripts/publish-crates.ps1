param(
    [switch]$DryRun,
    [switch]$AllowDirty
)

$dryRunArg = ""
if ($DryRun) {
    $dryRunArg = "--dry-run"
    Write-Host "Running in DRY-RUN mode"
}

$allowDirtyArg = ""
if ($AllowDirty) {
    $allowDirtyArg = "--allow-dirty"
    Write-Host "Allowing dirty working tree"
}

$crates = @(
    "crates/tandem-types",
    "crates/tandem-wire",
    "crates/tandem-observability",
    "crates/tandem-memory",
    "crates/tandem-skills",
    "crates/tandem-tools",
    "crates/tandem-providers",
    "crates/tandem-orchestrator",
    "crates/tandem-core",
    "crates/tandem-runtime",
    "crates/tandem-server",
    "crates/tandem-tui",
    "engine"
)

Write-Host "Publishing crates in order..."

foreach ($crate in $crates) {
    if (-not (Test-Path $crate)) {
        Write-Host "Skipping missing directory: $crate"
        continue
    }

    Write-Host "---------------------------------------------------"
    Write-Host "Processing $crate"

    $cargoToml = Join-Path $crate "Cargo.toml"
    $pathDeps = Select-String -Path $cargoToml -Pattern 'path\s*=' -SimpleMatch -Quiet
    if ($pathDeps) {
        Write-Host "Error: $crate contains local 'path' dependencies."
        Write-Host "Crates.io does not allow 'path' dependencies."
        Write-Host "Please replace 'path = ""...""' with version dependencies."
        Select-String -Path $cargoToml -Pattern 'path\s*='
        $answer = Read-Host "Continue anyway (local install)? [y/N]"
        if ($answer -notmatch '^[Yy]$') {
            exit 1
        }
    }

    Write-Host "Publishing..."
    Push-Location $crate
    try {
        $publishArgs = @()
        if ($dryRunArg) { $publishArgs += $dryRunArg }
        if ($allowDirtyArg) { $publishArgs += $allowDirtyArg }
        $output = & cargo publish @publishArgs 2>&1
        $exitCode = $LASTEXITCODE
        $output | ForEach-Object { Write-Host $_ }

        if ($exitCode -ne 0) {
            if ($output -match 'already exists on crates.io index') {
                Write-Host "Skipping $crate (already published)."
                continue
            }
            throw "Failed to publish $crate"
        }
    } finally {
        Pop-Location
    }

    Write-Host "Waiting 10s for propagation..."
    Start-Sleep -Seconds 10
}

Write-Host "All crates published!"
