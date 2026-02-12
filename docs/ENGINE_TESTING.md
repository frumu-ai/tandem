# Engine Build, Run, and Test Guide

This guide covers:

- how to build and start `tandem-engine`
- how to run automated tests
- how to run the end-to-end smoke/runtime proof flow on Windows, macOS, and Linux

## Quick commands

From `tandem/`:

```bash
cargo build -p tandem-engine
cargo run -p tandem-engine -- serve --host 127.0.0.1 --port 3000
cargo test -p tandem-server -p tandem-core -p tandem-engine
```

## CLI flags

`serve` supports:

- `--host` or `--hostname` (same option)
- `--port`
- `--state-dir`

State directory resolution order:

1. `--state-dir`
2. `TANDEM_STATE_DIR`
3. `.tandem`

## Automated test layers

## 1) Rust unit/integration tests (fast, CI-friendly)

Run:

```bash
cargo test -p tandem-server -p tandem-core -p tandem-engine
```

Coverage includes route shape/contracts like:

- `/global/health`
- `/provider`
- `/api/session` alias behavior
- `/session/{id}/message`
- SSE `message.part.updated`
- permission approve/deny compatibility routes

## 2) Engine smoke/runtime proof (process + HTTP + SSE + memory)

This is the automated version of the manual proof steps and writes artifacts to `runtime-proof/`.

### Windows (PowerShell)

```powershell
./scripts/engine_smoke.ps1
```

Optional args:

```powershell
./scripts/engine_smoke.ps1 -HostName 127.0.0.1 -Port 3000 -StateDir .tandem-smoke -OutDir runtime-proof
```

### macOS/Linux (bash)

Prerequisites:

- `jq`
- `curl`
- `ps`
- `pkill`

Run:

```bash
bash ./scripts/engine_smoke.sh
```

Optional env vars:

```bash
HOSTNAME=127.0.0.1 PORT=3000 STATE_DIR=.tandem-smoke OUT_DIR=runtime-proof bash ./scripts/engine_smoke.sh
```

## What smoke scripts validate

- engine starts and becomes healthy
- session create/list endpoints
- session message list endpoint has entries
- provider catalog endpoint
- SSE stream emits `message.part.updated`
- idle memory sample after 60s
- peak memory during tool-using prompt (with permission reply)
- cleanup leaves no rogue `tandem-engine` process

## Starting engine manually

### Windows

```powershell
cargo run -p tandem-engine -- serve --host 127.0.0.1 --port 3000 --state-dir .tandem
```

### macOS/Linux

```bash
cargo run -p tandem-engine -- serve --host 127.0.0.1 --port 3000 --state-dir .tandem
```

## Build commands by OS

### Windows

```powershell
cargo build -p tandem-engine
```

### macOS/Linux

```bash
cargo build -p tandem-engine
```

## Rogue process cleanup

### Windows

```powershell
Get-Process | Where-Object { $_.ProcessName -like 'tandem-engine*' } | Stop-Process -Force
```

### macOS/Linux

```bash
pkill -f tandem-engine || true
```

## Troubleshooting

- `Access is denied (os error 5)` on Windows build usually means `tandem-engine.exe` is still running and locked by the OS loader.
- Stop rogue engine processes, then rebuild.
- If bind fails, verify no process is already listening on your port.
- For writable state/config, use `--state-dir` with a project-local directory.
