# Crate File Size Refactor Kanban

Last updated: 2026-04-17

Purpose: reduce oversized Rust files in `crates/` to a safer size envelope and enforce
line-count governance in CI so future growth is controlled.

- Policy window:
  - Hard gate: touched file exceeds `2,000` lines
  - Target warning band: `>= 1,800` lines
  - Execution objective: bring all baseline files below `1,800` where practical
- Baseline scope: `64` Rust files in `crates/` at `>=1,500` lines
- Baseline artifact: [`crate-file-size-baseline.md`](../crate-file-size-baseline.md)

## Now

- [x] Compile-verify the split boundaries with `cargo build -p tandem-ai --profile fast-release`.
- [x] Add and activate CI line-count guard for touched `.rs/.tsx` files.
  - `scripts/ci-file-size-check.sh`
  - `.github/workflows/ci.yml`
- [x] Split `crates/tandem-server/src/http/coder.rs` into `include!` parts under `crates/tandem-server/src/http/coder_parts/` (max part size `1920`).
- [x] Split `crates/tandem-server/src/http/context_runs.rs` into `include!` parts under `crates/tandem-server/src/http/context_runs_parts/` (max part size `1944`).
- [x] Split `crates/tandem-server/src/http/routines_automations.rs` into `include!` parts under `crates/tandem-server/src/http/routines_automations_parts/` (max part size `1928`).
- [x] Split `crates/tandem-server/src/http/skills_memory.rs` into `include!` parts under `crates/tandem-server/src/http/skills_memory_parts/` (max part size `1930`).
- [x] Split `crates/tandem-server/src/http/config_providers.rs` into `include!` parts under `crates/tandem-server/src/http/config_providers_parts/` (max part size `1947`).
- [x] Split `crates/tandem-server/src/http/bug_monitor.rs` into `include!` parts under `crates/tandem-server/src/http/bug_monitor_parts/` (max part size `1916`).
- [x] Split `crates/tandem-server/src/http/workflow_planner.rs` into `include!` parts under `crates/tandem-server/src/http/workflow_planner_parts/` (max part size `1186`).
- [x] Split `crates/tandem-server/src/browser.rs` into `include!` parts under `crates/tandem-server/src/browser_parts/` (max part size `1282`).
- [x] Split `crates/tandem-server/src/pack_builder.rs` into `include!` parts under `crates/tandem-server/src/pack_builder_parts/` (max part size `1950`).
- [x] Split large `impl MemoryDatabase` in `crates/tandem-memory/src/db.rs` into includes under `crates/tandem-memory/src/memory_database_impl_parts/` (max part size `1889`).
- [x] Split `crates/tandem-server/src/agent_teams.rs` into `include!` parts under `crates/tandem-server/src/agent_teams_parts/` (max part size `1937`).
- [x] Split `crates/tandem-plan-compiler/src/workflow_plan.rs` into `include!` parts under `crates/tandem-plan-compiler/src/workflow_plan_parts/` (max part size `1921`).
- [x] Split `crates/tandem-server/src/http/tests/context_runs.rs` into `include!` parts under `crates/tandem-server/src/http/tests/context_runs_parts/` (max part size `1876`).
- [x] Split `crates/tandem-server/src/http/tests/bug_monitor.rs` into `include!` parts under `crates/tandem-server/src/http/tests/bug_monitor_parts/` (max part size `1780`).
- [x] Split `crates/tandem-server/src/http/tests/optimizations.rs` into `include!` parts under `crates/tandem-server/src/http/tests/optimizations_parts/` (max part size `1844`).
- [x] Split `crates/tandem-server/src/http/tests/workflow_planner.rs` into `include!` parts under `crates/tandem-server/src/http/tests/workflow_planner_parts/` (max part size `1945`).
- [x] Split `crates/tandem-server/src/http/tests/global.rs` into `include!` parts under `crates/tandem-server/src/http/tests/global_parts/` (max part size `1932`).
- [x] Split `crates/tandem-server/src/http/tests/coder.rs` into `include!` parts under `crates/tandem-server/src/http/tests/coder_parts/` (max part size `1944`).
- [x] Split `crates/tandem-server/src/http/tests/memory.rs` into `include!` parts under `crates/tandem-server/src/http/tests/memory_parts/` (max part size `1268`).
- [x] Split `crates/tandem-server/src/http/tests/sessions.rs` into `include!` parts under `crates/tandem-server/src/http/tests/sessions_parts/` (max part size `1101`).
- [x] Split `crates/tandem-server/src/app/state/tests/automations.rs` into `include!` parts under `crates/tandem-server/src/app/state/tests/automations_parts/` (max part size `1881`).
- [x] Split `crates/tandem-server/src/app/state/tests/automations/validation.rs` into `include!` parts under `crates/tandem-server/src/app/state/tests/automations/validation_parts/` (max part size `1922`).
- [x] Split `crates/tandem-server/src/app/state/tests/automations/prompting.rs` into `include!` parts under `crates/tandem-server/src/app/state/tests/automations/prompting_parts/` (max part size `1447`).
- [x] Split `crates/tandem-server/src/app/state/tests/automations/integration.rs` into modules under `crates/tandem-server/src/app/state/tests/automations/integration_parts/` (max part size `726`).
- [x] Split `crates/tandem-server/src/app/state/tests/automations/workflow_policy.rs` into `include!` parts under `crates/tandem-server/src/app/state/tests/automations/workflow_policy_parts/` (max part size `1291`).
- [x] Split `crates/tandem-tui/src/app.rs` by extracting `impl App` into `include!` parts under `crates/tandem-tui/src/app_impl_parts/` and splitting `App::update` match arms under `crates/tandem-tui/src/app_update_match_arms_parts/` (max part size `1876`).
- [x] Split `crates/tandem-tui/src/net/client.rs` into `include!` parts under `crates/tandem-tui/src/net/client_parts/` (max part size `1120`).
- [x] Split `crates/tandem-tui/src/app/commands.rs` by extracting match arms into `include!` parts under `crates/tandem-tui/src/app/commands_parts/` (max part size `1193`).
- [x] Split `crates/tandem-providers/src/lib.rs` into `include!` parts under `crates/tandem-providers/src/lib_parts/` (max part size `1224`).
- [x] Split `crates/tandem-tools/src/lib.rs` into `include!` parts under `crates/tandem-tools/src/lib_parts/` (max part size `1441`).
- [x] Split `crates/tandem-channels/src/dispatcher.rs` into `include!` parts under `crates/tandem-channels/src/dispatcher_parts/` (max part size `1435`).
- [x] Split `crates/tandem-runtime/src/mcp.rs` into `include!` parts under `crates/tandem-runtime/src/mcp_parts/` (max part size `979`).
- [x] Split `crates/tandem-server/src/app/state/mod.rs` by extracting `impl AppState` into `include!` parts under `crates/tandem-server/src/app/state/app_state_impl_parts/` (max part size `1901`).
- [x] Split `crates/tandem-plan-compiler/src/plan_package.rs` into `include!` parts under `crates/tandem-plan-compiler/src/plan_package_parts/` (max part size `1201`).
- [x] Split `crates/tandem-plan-compiler/src/plan_validation.rs` into `include!` parts under `crates/tandem-plan-compiler/src/plan_validation_parts/` (max part size `1355`).
- [x] Split `crates/tandem-memory/src/manager.rs` into `include!` parts under `crates/tandem-memory/src/manager_parts/` (max part size `1247`).
- [ ] Split `crates/tandem-core/src/engine_loop.rs` into focused execution submodules.
  - [x] Extract shared public tool types and hook traits into `crates/tandem-core/src/engine_loop/types.rs`.
  - [x] Extract tool execution/session context helpers into `crates/tandem-core/src/engine_loop/tool_execution.rs`.
  - [x] Extract prompt execution loop into `crates/tandem-core/src/engine_loop/prompt_execution.rs`.
  - [x] Extract prompt/runtime orchestration helpers into `crates/tandem-core/src/engine_loop/prompt_runtime.rs`.
  - [x] Extract prompt helper/policy utilities into `crates/tandem-core/src/engine_loop/prompt_helpers.rs`.
  - [x] Extract engine-loop tests into `crates/tandem-core/src/engine_loop/tests.rs`.
  - [x] Extract tool-arg normalization/inference into `crates/tandem-core/src/engine_loop/tool_parsing/normalize.rs`.
  - [ ] Continue splitting engine-loop execution logic into additional domain modules.
  - [x] Reduce `crates/tandem-core/src/engine_loop.rs` from `3176` lines to below the `2000` hard cap (now `1183`).
  - [x] Reduce `crates/tandem-core/src/engine_loop/tool_parsing.rs` from `2484` lines to below the `2000` hard cap (now `778`).
  - [x] Split `crates/tandem-core/src/engine_loop/tests.rs` into suites under `crates/tandem-core/src/engine_loop/tests/`.
  - [x] Reduce `crates/tandem-core/src/engine_loop/tests.rs` from `2279` lines to below the `2000` hard cap (now `17`).
- [x] Split `crates/tandem-server/src/app/state/automation/mod.rs` into a thin wiring layer and preserve public API via re-exports.
- [x] Split `crates/tandem-server/src/app/state/automation/node_output.rs` into `include!` parts under `crates/tandem-server/src/app/state/automation/node_output_parts/` (max part size `1475`).
- [x] Split `crates/tandem-server/src/app/state/automation/logic.rs` (8660 lines) into includes under `crates/tandem-server/src/app/state/automation/logic_parts/` (max part size `1935`).
- [x] Split `crates/tandem-server/src/app/state/automation/logic/part03.rs` into `include!` parts under `crates/tandem-server/src/app/state/automation/logic/part03_parts/` (max part size `1517`).
- [x] Split `crates/tandem-server/src/app/state/automation/logic/part04.rs` into `include!` parts under `crates/tandem-server/src/app/state/automation/logic/part04_parts/` (max part size `1308`).
- [x] Split `crates/tandem-server/src/app/state/automation/logic/part01.rs` into `include!` parts under `crates/tandem-server/src/app/state/automation/logic/part01_parts/` (max part size `993`).
- [x] Split `crates/tandem-server/src/app/state/automation/tests.rs` into `include!` parts under `crates/tandem-server/src/app/state/automation/tests_parts/` (max part size `1277`).
- [ ] Optional follow-up: rename `logic_parts/part*.rs` into domain-named modules once boundaries stabilize.
- [ ] Split `crates/tandem-server/src/app/state/mod.rs` around state graph, persistence, and orchestration concerns.
- [ ] Split `crates/tandem-server/src/app/state/tests/automations/workflow_policy.rs` into test suites by failure mode and scenario group.

## Next

- [ ] Split `crates/tandem-server/src/http/routines_automations.rs` and re-export existing API through a stable module boundary.
- [ ] Split `crates/tandem-channels/src/dispatcher.rs` and remove monolithic branching in dispatch handling.
- [ ] Split `crates/tandem-tui/src/app.rs` into UI controller and handler modules without changing command surfaces.
- [ ] Split `crates/tandem-tools/src/lib.rs` and `crates/tandem-tui/src/app/commands.rs`.
- [ ] Split `crates/tandem-server/src/app/state/tests/automations.rs`, `crates/tandem-server/src/http/tests/global.rs`, and `crates/tandem-server/src/http/tests/workflow_planner.rs`.

## After That

- [ ] Split remaining 1,500–1,999-line files or explicitly defer with a rationale in this board.
- [ ] Add periodic baseline refresh verification in pre-merge checks for `crates/` only.
- [ ] Keep `crate-file-size-baseline.md` updated as files are reduced, and annotate each completed split with the new module set.

## Risks

- [ ] Refactoring high-surface files can hide subtle behavior changes, especially around shared fixtures and integration pathways.
- [ ] Test-heavy files may gain new compile-time coupling if module visibility boundaries are not explicit.
- [ ] The CI gate can create friction for generated files if future codegen output is written inside scope.
- [ ] Over-splitting can introduce circular module dependency if ownership and interfaces are not finalized first.

## Completion Signal

This track is complete when:

- No `git merge`/`pull_request` pipeline run fails `scripts/ci-file-size-check.sh` for the 2,000-line hard cap.
- Each baseline file `>= 2,000` lines has been split (or has a recorded exception) and tracked on this board.
- No remaining `crates/` files are above `2,000` lines.
- The baseline file at [`crate-file-size-baseline.md`](../crate-file-size-baseline.md) is refreshed and tracked as changed only by planned split work.
