# Engine Build, Run, and Test Guide

This guide covers:

- how to build and start `tandem-engine`
- how to run automated tests
- how to run the end-to-end smoke/runtime proof flow on Windows, macOS, and Linux

## Windows quickstart (engine + tauri dev)

From `tandem/`:

```powershell
pnpm install
pnpm engine:stop:windows
cargo build -p tandem-ai
New-Item -ItemType Directory -Force -Path .\src-tauri\binaries | Out-Null
Copy-Item .\target\debug\tandem-engine.exe .\src-tauri\binaries\tandem-engine.exe -Force
pnpm tauri dev
```

## macOS/Linux quickstart (engine + tauri dev)

From `tandem/`:

```bash
pnpm install
# Kill any existing engine instance
pkill tandem-engine || true
cargo build -p tandem-ai
mkdir -p src-tauri/binaries
cp target/debug/tandem-engine src-tauri/binaries/tandem-engine
pnpm tauri dev
```

PowerShell equivalent (Windows):

```powershell
pnpm install
Get-Process | Where-Object { $_.ProcessName -in @('tandem-engine','tandem') } | Stop-Process -Force -ErrorAction SilentlyContinue
cargo build -p tandem-ai
New-Item -ItemType Directory -Force -Path .\src-tauri\binaries | Out-Null
Copy-Item .\target\debug\tandem-engine.exe .\src-tauri\binaries\tandem-engine.exe -Force
pnpm tauri dev
```

## Quick commands

From `tandem/`:

```bash
cargo build -p tandem-ai
cargo run -p tandem-ai -- serve --host 127.0.0.1 --port 39731
cargo test -p tandem-server -p tandem-core -p tandem-ai
```

# Testing with packages/tandem-control-panel

```bash
cargo build -p tandem-ai --profile fast-release
sudo install -m 755 target/fast-release/tandem-engine /usr/local/bin/tandem-engine
sudo systemctl restart tandem-engine
```

# Control panel testing locally

```bash
cd packages/tandem-control-panel && pnpm build && sudo systemctl restart tandem-control-panel.service && cd ../..
```

## API Token Security Validation

Verify token-gated API behavior:

```bash
cargo run -p tandem-ai -- serve --host 127.0.0.1 --port 39731 --state-dir .tandem --api-token tk_test_token
```

In a second terminal:

```bash
# Health remains public
curl -s http://127.0.0.1:39731/global/health | jq .

# Non-health endpoints require token
curl -i -s http://127.0.0.1:39731/config/providers
curl -s http://127.0.0.1:39731/config/providers -H "X-Tandem-Token: tk_test_token" | jq .
```

Desktop/TUI token checks:

- Desktop Settings now shows `Engine API Token` masked by default with `Reveal` and `Copy`.
- TUI supports `/engine token` for masked output and `/engine token show` for full output plus storage backend/path.
- Storage backend is reported as `keychain`, `file`, `env`, or `memory`.

## OS-Aware Runtime Validation

Validate that engine-detected environment is surfaced to every client path:

```bash
curl -s http://127.0.0.1:39731/global/health | jq .environment
```

Expected fields:

- `os`
- `shell_family`
- `path_style`
- `arch`

Optional: toggle environment prompt block:

```bash
TANDEM_OS_AWARE_PROMPTS=0 cargo run -p tandem-ai -- serve --host 127.0.0.1 --port 39731
```

On Windows, validate shell guardrails:

- Unix-only commands that cannot be safely translated are blocked with `metadata.os_guardrail_applied=true` and `guardrail_reason`.
- Translatable commands (for example `ls -la`, `find ... -type f -name ...`) include `metadata.translated_command`.

## CLI flags

`serve` supports:

- `--host` or `--hostname` (same option)
- `--port`
- `--state-dir`

State directory resolution order:

1. `--state-dir`
2. `TANDEM_STATE_DIR`
3. canonical shared storage data dir (`.../tandem/data`)

## Tool testing (CLI)

Use the `tool` subcommand to invoke built-in tools directly with JSON input.
`webfetch` is especially useful because it converts noisy HTML into clean Markdown,
extracts links + metadata, and reports size reductions. It should work against any public
HTTP/HTTPS webpage (subject to site limits, auth, or anti-bot protections).
Use `webfetch_html` when raw HTML is explicitly required.

The Markdown output is returned inline on stdout as JSON in the `output` field:

- `output.markdown` holds the Markdown
- `output.text` holds the plain-text fallback
- `output.stats` includes raw vs Markdown size

Size savings example (proven from the Frumu.ai run above):

- raw chars: 36,141
- markdown chars: 7,292
- reduction: 79.82%
- bytes in: 36,188
- bytes out: 7,292

### Windows (PowerShell)

```powershell
@'
{"tool":"webfetch","args":{"url":"https://frumu.ai","return":"both","mode":"auto"}}
'@ | cargo run -p tandem-ai -- tool --json -
```

```powershell
@'
{"tool":"mcp_debug","args":{"url":"https://mcp.exa.ai/mcp","tool":"web_search_exa","args":{"query":"tandem engine","numResults":1}}}
'@ | cargo run -p tandem-ai -- tool --json -
```

### macOS/Linux (bash)

```bash
cat << 'JSON' | cargo run -p tandem-ai -- tool --json -
{"tool":"webfetch","args":{"url":"https://frumu.ai","return":"both","mode":"auto"}}
JSON
```

## Automated test layers

## 1) Rust unit/integration tests (fast, CI-friendly)

Run:

```bash
cargo test -p tandem-server -p tandem-core -p tandem-ai
```

Coverage includes route shape/contracts like:

- `/global/health`
- `/provider`
- `/api/session` alias behavior
- `/mission` create/list/get/apply-event
- `/routines` create/list/patch/delete/run-now/history/events
- `/session/{id}/message`
- `/session/{id}/run`
- `/session/{id}/run/{run_id}/cancel`
- SSE `message.part.updated`
- `prompt_async?return=run` (`202` with `runID` + attach stream)
- same-session conflict (`409` with nested `activeRun`)
- permission approve/deny compatibility routes

Mission/routine policy-focused tests:

```bash
cargo test -p tandem-server mission_ -- --nocapture
cargo test -p tandem-server routines_ -- --nocapture
cargo test -p tandem-server routine_policy_ -- --nocapture
cargo test -p tandem-server routines_run_now_ -- --nocapture
```

Contract-focused tests (JSON-first orchestrator parsing):

```bash
cargo test -p tandem test_parse_task_list_strict -- --nocapture
cargo test -p tandem test_parse_validation_result_strict_rejects_prose -- --nocapture
```

## Workflow test recovery loop

When working on workflow runtime correctness, use this loop:

1. Reproduce the issue from live workflow output or `error.log`.
2. Add or extend a regression test before changing the runtime.
3. Fix the runtime behavior.
4. Re-run the fast tandem-server workflow baseline.
5. Run a targeted deeper test only after the baseline is green.

Recommended commands from `tandem/`:

```bash
# First recovery gate: ensure tandem-server workflow tests compile
cargo test -p tandem-server --lib --tests --no-run

# Fast validation after focused workflow-runtime edits
cargo check -p tandem-server --lib

# Focused workflow policy / validation areas
cargo test -p tandem-server workflow_policy -- --nocapture
cargo test -p tandem-server validation_ -- --nocapture

# Broad tandem-server test run once the baseline is healthy
cargo test -p tandem-server --lib --tests
```

Workflow-specific expectations:

- Treat `cargo test -p tandem-server --lib --tests --no-run` as the minimum required health check before claiming workflow test coverage is restored.
- Prefer regression tests for contradictions between validator state, stored run state, projected backlog state, and API-visible status.
- If a focused named test is blocked by an unrelated workspace dependency compile error, record that blocker explicitly and keep the tandem-server no-run baseline green while the dependency issue is fixed.

Current fast-gate invariant candidates:

```bash
cargo test -p tandem-server --lib --tests --no-run --message-format=json
# then invoke the tandem_server test binary directly with:
target/debug/deps/tandem_server-* 'app::state::tests::automations::recover_in_flight_runs_does_not_relock_workspace_for_paused_runs' --exact --nocapture
target/debug/deps/tandem_server-* 'http::tests::global::automation_v2_run_projects_backlog_tasks_into_context_blackboard' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::workflow_policy::mcp_grounded_citations_artifact_passes_without_local_reads_or_websearch' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::workflow_policy::materialized_current_attempt_output_does_not_report_missing_output_requirement' --exact --nocapture
```

Current deep-gate workflow coverage:

```bash
target/debug/deps/tandem_server-* 'app::state::tests::automations::workflow_policy::research_brief_passes_local_only_when_websearch_is_not_offered' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::workflow_policy::research_citations_validation_accepts_external_research_without_files_reviewed_section' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::workflow_policy::missing_required_output_requests_repair_before_attempt_budget_is_exhausted' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::integration::local_research_flow_completes_with_read_and_write_artifact' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::integration::mcp_grounded_research_flow_completes_with_mcp_tool_usage' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::integration::external_web_research_flow_completes_with_websearch_and_write' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::integration::analyze_findings_dual_write_flow_completes_with_artifact_and_workspace_file' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::integration::compare_results_synthesis_flow_completes_with_upstream_evidence' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::integration::delivery_flow_completes_with_validated_artifact_body_and_email_send' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::integration::code_loop_flow_repairs_after_missing_verification_and_completes' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::integration::repair_retry_after_needs_repair_completes_on_second_attempt' --exact --nocapture
target/debug/deps/tandem_server-* 'app::state::tests::automations::integration::restart_recovery_preserves_queued_and_paused_runs' --exact --nocapture
target/debug/deps/tandem_server-* 'http::tests::global::automations_v2_run_recover_from_stale_pause_clears_pending_outputs_and_attempts' --exact --nocapture
target/debug/deps/tandem_server-* 'http::tests::global::automations_v2_run_pause_clears_active_sessions_and_instances' --exact --nocapture
target/debug/deps/tandem_server-* 'http::tests::global::automations_v2_run_cancel_records_operator_stop_kind_and_clears_active_ids' --exact --nocapture
```

## Release-candidate workflow checklist

Before bumping a version or cutting a workflow-runtime release candidate:

1. Fast gate is green.
   - At minimum:

```bash
cargo test -p tandem-server --lib --tests --no-run --message-format=json
```

2. Deep gate is green.
   - Run the focused workflow subset above, including:
     - replay regressions for the latest escaped failures
     - synthesis integrations
     - delivery integration
     - code-loop integration

3. Targeted replay coverage exists for every workflow-runtime bug fixed since the last release.
   - Do not ship a runtime fix that only exists in code without a replay regression.

4. If the release includes workflow enforcement, prompting, validation, or repair logic changes:
   - rerun the exact named tests touched by that change locally before tagging
   - do not rely on generic `cargo test` alone

5. If a release candidate is blocked by an unrelated compile failure outside the touched workflow path:
   - keep the `tandem-server --no-run` baseline green
   - record the unrelated blocker explicitly in the release notes or handoff
   - do not mark workflow coverage restored unless the focused workflow subset is also green

## Workflow bug replay loop

For workflow-runtime failures, follow the replay process in [Workflow Bug Replay Guide](./WORKFLOW_BUG_REPLAY.md).

Short rule:

- do not merge or release a workflow-runtime fix unless a deterministic replay regression exists
- prefer the narrowest replay test that protects the bug class
- if the escaped bug is release-relevant, add that replay to the deep gate or targeted release subset

Desktop/CLI runtime contract closure tests:

```bash
# Desktop sidecar tests (includes reconnect recovery + conflict parsing + run-id cancel)
cargo test -p tandem sidecar::tests::recover_active_run_attach_stream_uses_get_run_endpoint -- --nocapture
cargo test -p tandem sidecar::tests::test_parse_prompt_async_response_409_includes_retry_and_attach -- --nocapture
cargo test -p tandem sidecar::tests::cancel_run_by_id_posts_expected_endpoint -- --nocapture
cargo test -p tandem sidecar::tests::mission_list_reads_engine_missions_endpoint -- --nocapture
cargo test -p tandem sidecar::tests::mission_get_reads_engine_mission_endpoint -- --nocapture
cargo test -p tandem sidecar::tests::mission_create_posts_to_engine_mission_endpoint -- --nocapture
cargo test -p tandem sidecar::tests::mission_apply_event_posts_event_payload -- --nocapture

# CLI (tandem-tui) run-id cancel client path
cargo test -p tandem-tui cancel_run_by_id_posts_expected_endpoint -- --nocapture
```

Example MCP auth challenge shape:

```text
Authorization is required before I can continue with this action.

Tool mcp.arcade.jira_getboards result: Authorization required for mcp.arcade.jira_getboards.
Authorize here: https://example.com/oauth/start
```

Use a sanitized example in docs. Do not paste live OAuth challenge payloads with real redirect URIs, scopes, or state values into committed documentation.
MCP-focused regression checks:

```bash
# Runtime MCP auth challenge extraction + schema normalization coverage
cargo test -p tandem-runtime mcp::tests::extract_auth_challenge_from_result_payload -- --nocapture
cargo test -p tandem-runtime mcp::tests::normalize_mcp_tool_args_maps_clickup_aliases -- --nocapture

# Full runtime MCP test module
cargo test -p tandem-runtime mcp::tests -- --nocapture
```

Manual MCP smoke checks:

1. Connect MCP server and verify tools are present in `/tool`.
2. Trigger an auth-gated tool and verify `mcp.auth.required` is emitted.
3. Complete auth and retry tool call (no engine restart required).
4. Force refresh/disconnect failure and verify stale MCP tools are not left active.
5. In web quickstart, verify run failures are rendered (no blank chat state on failure).

TUI mission quick-action commands (manual runtime validation):

```text
/missions
/mission_create Mission Demo :: Validate quick actions :: Initial task
/mission_get <mission_id>
/mission_start <mission_id>
/mission_review_ok <mission_id> <work_item_id>
/mission_test_ok <mission_id> <work_item_id>
/mission_review_no <mission_id> <work_item_id> needs_revision
```

## 2) Engine smoke/runtime proof (process + HTTP + SSE + memory)

This is the automated version of the manual proof steps and writes artifacts to `runtime-proof/`.

### Windows (PowerShell)

```powershell
./scripts/engine_smoke.ps1
```

Optional args:

```powershell
./scripts/engine_smoke.ps1 -HostName 127.0.0.1 -Port 39731 -StateDir .tandem-smoke -OutDir runtime-proof
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
HOSTNAME=127.0.0.1 PORT=39731 STATE_DIR=.tandem-smoke OUT_DIR=runtime-proof bash ./scripts/engine_smoke.sh
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
cargo run -p tandem-ai -- serve --host 127.0.0.1 --port 39731 --state-dir .tandem
```

## Running with `pnpm tauri dev`

Tauri dev must be able to find the `tandem-engine` sidecar binary in a dev lookup path.
Use the binary built in `target/` and copy it into `src-tauri/binaries/`.

Important: the filename is the same (`tandem-engine` or `tandem-engine.exe`), but the directories are different.

## Shared Engine Mode (Desktop + TUI together)

Desktop and TUI now use shared engine mode by default:

- default engine port: `127.0.0.1:39731`
- clients attach to an already-running engine when available
- closing one client detaches instead of force-stopping the shared engine
- set `TANDEM_ENGINE_PORT` to override the shared default for both desktop and TUI

Disable shared mode (legacy single-client behavior) by setting:

```powershell
$env:TANDEM_SHARED_ENGINE_MODE="0"
```

If the app is stuck on `Connecting...` or fails to load, do a clean dev restart:

```powershell
pnpm tauri:dev:clean
```

Manual equivalent:

```powershell
Get-Process | Where-Object { $_.ProcessName -in @('tandem','tandem-engine','node') } | Stop-Process -Force -ErrorAction SilentlyContinue
pnpm tauri dev
```

## JSON-first orchestrator feature flag

Strict planner/validator contract mode can be forced via env:

### Windows (PowerShell)

```powershell
$env:TANDEM_ORCH_STRICT_CONTRACT="1"
pnpm tauri dev
```

### macOS/Linux (bash)

```bash
TANDEM_ORCH_STRICT_CONTRACT=1 pnpm tauri dev
```

Behavior in strict mode:

- planner uses strict JSON parse first
- validator uses strict JSON parse first
- prose fallback is still allowed by default (`allow_prose_fallback=true`) during phase 1
- contract degradation/failures emit `contract_warning` / `contract_error` orchestrator events

### macOS/Linux (bash)

From `tandem/`:

```bash
cargo build -p tandem-ai
mkdir -p ./src-tauri/binaries
cp ./target/debug/tandem-engine ./src-tauri/binaries/tandem-engine
chmod +x ./src-tauri/binaries/tandem-engine
pnpm tauri dev
```

### macOS/Linux

```bash
cargo run -p tandem-ai -- serve --host 127.0.0.1 --port 39731 --state-dir .tandem
```

## Build commands by OS

### Windows

```powershell
cargo build -p tandem-ai
```

### macOS/Linux

```bash
cargo build -p tandem-ai
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
- If startup log shows `Another instance tried to launch`, a previous app instance is still running. Close/kill all `tandem` processes and relaunch.
- For writable state/config, use `--state-dir` with a project-local directory.

Windows copy/paste recovery for `os error 5`:

```powershell
Get-Process | Where-Object { $_.ProcessName -in @('tandem-engine','tandem') } | Stop-Process -Force -ErrorAction SilentlyContinue
cargo build -p tandem-ai
New-Item -ItemType Directory -Force -Path .\src-tauri\binaries | Out-Null
Copy-Item .\target\debug\tandem-engine.exe .\src-tauri\binaries\tandem-engine.exe -Force
```

Note: `mkdir -p` and `cp target/debug/tandem-engine ...` are macOS/Linux commands. On Windows, use `New-Item` and `Copy-Item` with the `.exe` filename.

## Storage migration verification (legacy upgrades)

When upgrading from builds that used `ai.frumu.tandem`, verify canonical migration:

1. Ensure `%APPDATA%/ai.frumu.tandem` exists with legacy data.
2. Ensure `%APPDATA%/tandem` is empty or absent.
3. Start Tandem app or engine once.
4. Verify `%APPDATA%/tandem/storage_version.json` and `%APPDATA%/tandem/migration_report.json` exist.
5. Verify sessions/tool history are visible without manual copying.
6. Verify `%APPDATA%/ai.frumu.tandem` remains intact (copy + keep legacy).

## Startup migration wizard verification (blocking progress UX)

1. Seed legacy data under either:
2. `%APPDATA%/ai.frumu.tandem`
3. `%APPDATA%/opencode`
4. `%USERPROFILE%/.local/share/opencode/storage`
5. Launch Tandem and unlock vault.
6. Verify full-screen migration overlay appears and blocks interaction until completion.
7. Verify progress updates through phases:
8. scanning -> copying -> rehydrating -> finalizing.
9. On success/partial, verify summary card shows repaired session/message counts.
10. Click Continue and verify chat history loads for migrated sessions.

## Settings migration rerun verification

1. Open Settings -> Data Migration.
2. Run `Dry Run` and verify result status reports `dry_run`.
3. Run `Run Migration Again` and verify counters update.
4. Verify `migration_report.json` timestamp updates and report path is shown.

## Workspace namespace migration verification (`.opencode` -> `.tandem`)

For an existing workspace that contains legacy metadata:

1. Ensure `<workspace>/.opencode/plans` and/or `<workspace>/.opencode/skill` exists.
2. Start Tandem and set/switch active workspace to that folder.
3. Verify `<workspace>/.tandem/plans` and `<workspace>/.tandem/skill` are created.
4. Verify plan list and skills list still include legacy entries.
5. Create a new plan and install/import a skill; verify new files are written under `.tandem/*`.
6. Confirm legacy `.opencode/*` remains untouched (read-compatible window).

## Workspace-scoped session history checks

1. With multiple projects configured, switch active folder in the project switcher.
2. Verify sidebar session list shows only sessions belonging to that folder.
3. Create a new chat in the active folder; verify it appears immediately in sidebar.
4. Verify sessions with legacy `directory = "."` still appear under current workspace.

## Observability verification (JSONL + correlation)

After launching desktop (`pnpm tauri dev`) and sending one prompt:

1. Open `%APPDATA%\\tandem\\logs` and verify files exist:
   - `tandem.desktop.YYYY-MM-DD.jsonl`
   - `tandem.engine.YYYY-MM-DD.jsonl`
2. Search for `provider.call.start` in engine JSONL.
3. Search for `chat.dispatch.start` in desktop JSONL.
4. Verify a matching `correlation_id` exists across desktop dispatch and engine provider events.
5. If stream fails, verify one of:
   - `stream.subscribe.error`
   - `stream.disconnected`
   - `stream.watchdog.no_events`

PowerShell helpers:

```powershell
Select-String -Path "$env:APPDATA\tandem\logs\tandem.desktop.*.jsonl" -Pattern "chat.dispatch.start"
Select-String -Path "$env:APPDATA\tandem\logs\tandem.engine.*.jsonl" -Pattern "provider.call.start"
Select-String -Path "$env:APPDATA\tandem\logs\tandem.desktop.*.jsonl" -Pattern "stream.subscribe.error|stream.disconnected|stream.watchdog.no_events"
```
