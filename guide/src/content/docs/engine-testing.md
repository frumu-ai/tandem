---
title: Engine Testing
---

This page summarizes how to build, run, and validate Tandem Engine behavior across unit tests, smoke tests, and security checks.

## Quickstart (engine + tauri dev)

**Windows (PowerShell)** from `tandem/`:

```powershell
pnpm install
pnpm engine:stop:windows
cargo build -p tandem-ai
New-Item -ItemType Directory -Force -Path .\src-tauri\binaries | Out-Null
Copy-Item .\target\debug\tandem-engine.exe .\src-tauri\binaries\tandem-engine.exe -Force
pnpm tauri dev
```

**macOS/Linux (bash)** from `tandem/`:

```bash
pnpm install
pkill tandem-engine || true
cargo build -p tandem-ai
mkdir -p src-tauri/binaries
cp target/debug/tandem-engine src-tauri/binaries/tandem-engine
pnpm tauri dev
```

## Quick commands

```bash
cargo build -p tandem-ai
cargo run -p tandem-ai -- serve --host 127.0.0.1 --port 39731
cargo test -p tandem-server -p tandem-core -p tandem-ai
```

## API token validation

Start a token-gated engine:

```bash
cargo run -p tandem-ai -- serve --host 127.0.0.1 --port 39731 --state-dir .tandem --api-token tk_test_token
```

Then verify public health + gated routes:

```bash
curl -s http://127.0.0.1:39731/global/health | jq .
curl -i -s http://127.0.0.1:39731/config/providers
curl -s http://127.0.0.1:39731/config/providers -H "X-Tandem-Token: tk_test_token" | jq .
```

## Automated test layers

**Rust unit/integration tests**:

```bash
cargo test -p tandem-server -p tandem-core -p tandem-ai
```

**Mission/routine focused tests**:

```bash
cargo test -p tandem-server mission_ -- --nocapture
cargo test -p tandem-server routines_ -- --nocapture
cargo test -p tandem-server routine_policy_ -- --nocapture
cargo test -p tandem-server routines_run_now_ -- --nocapture
```

**JSON-first contract tests**:

```bash
cargo test -p tandem test_parse_task_list_strict -- --nocapture
cargo test -p tandem test_parse_validation_result_strict_rejects_prose -- --nocapture
```

**Sidecar runtime contract tests**:

```bash
cargo test -p tandem sidecar::tests::recover_active_run_attach_stream_uses_get_run_endpoint -- --nocapture
cargo test -p tandem sidecar::tests::test_parse_prompt_async_response_409_includes_retry_and_attach -- --nocapture
cargo test -p tandem sidecar::tests::cancel_run_by_id_posts_expected_endpoint -- --nocapture
```

## Engine smoke tests

**Windows**:

```powershell
./scripts/engine_smoke.ps1
```

**macOS/Linux**:

```bash
bash ./scripts/engine_smoke.sh
```

Optional overrides:

```bash
HOSTNAME=127.0.0.1 PORT=39731 STATE_DIR=.tandem-smoke OUT_DIR=runtime-proof bash ./scripts/engine_smoke.sh
```

## Shared Engine Mode

Desktop and TUI default to shared engine mode:

- default port `39731`
- clients attach to the same engine when available
- set `TANDEM_ENGINE_PORT` to override for both desktop and TUI

Disable shared mode (legacy single-client behavior):

```powershell
$env:TANDEM_SHARED_ENGINE_MODE="0"
```
