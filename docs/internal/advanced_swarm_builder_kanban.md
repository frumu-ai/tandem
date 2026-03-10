# Advanced Swarm Builder Kanban

Last updated: 2026-03-10
Source plan: `docs/internal/advanced_swarm_builder_plan.md`

## Slice Goal
Ship an advanced power-user mission compiler that lets operators define one coordinated swarm mission with shared mission context, role-based workstreams, explicit handoffs, per-lane model/template selection, hard approval gates, operator kill switches, and recoverable failure handling, while keeping the existing simple automation wizard intact.

## Status Legend
- [ ] Todo
- [~] In Progress
- [x] Done

## Done
- [x] Save the implementation plan in `docs/internal/advanced_swarm_builder_plan.md`.
- [x] Add shared `MissionBlueprint` model + validation in `crates/tandem-workflows/src/mission_builder.rs`.
- [x] Export mission-builder contracts from `crates/tandem-workflows/src/lib.rs`.
- [x] Add backend mission-builder preview/apply handlers in `crates/tandem-server/src/http/mission_builder.rs`.
- [x] Wire mission-builder routing through:
  - `crates/tandem-server/src/http/routes_mission_builder.rs`
  - `crates/tandem-server/src/http.rs`
  - `crates/tandem-server/src/http/router.rs`
- [x] Compile mission blueprints into `AutomationV2Spec` drafts with derived mission/work-item previews.
- [x] Extend `AutomationV2` contracts for:
  - stage kinds
  - richer output contracts
  - gate metadata
  - approval-state checkpoint data
- [x] Inject mission-wide brief, local assignment, and output-contract guidance into automation node prompts.
- [x] Add hard approval gate runtime handling for:
  - approve
  - rework
  - cancel
- [x] Add automation-v2 gate decision endpoint in `crates/tandem-server/src/http/routines_automations.rs`.
- [x] Add mission-builder backend tests in `crates/tandem-server/src/http/tests/mission_builder.rs`.
- [x] Add Tauri sidecar bridge methods for mission-builder preview/apply.
- [x] Add Tauri commands + registration for:
  - mission-builder preview/apply
  - automation-v2 gate decision submission
- [x] Add TS mission-builder contracts and invoke wrappers in `src/lib/tauri.ts`.
- [x] Add `AdvancedMissionBuilder` UI in `src/components/agent-automation/AdvancedMissionBuilder.tsx`.
- [x] Add `Simple Wizard` / `Advanced Swarm Builder` mode switching in `src/components/agent-automation/AgentAutomationPage.tsx`.
- [x] Add run-inspector gate actions so operators can approve, rework, or cancel blocked runs.
- [x] Add backend recovery handling for failed automation runs and subtree reset.
- [x] Add Tauri sidecar bridge + desktop command for automation-v2 run recovery.
- [x] Add mission-level guardrail fields in the advanced builder for tokens, cost, runtime, and tool-call ceilings.
- [x] Add per-step tool and MCP scope controls in the advanced builder for:
  - workstreams
  - review/test stages
- [x] Add operator diagnostics in the run inspector for:
  - token usage
  - estimated cost
  - tool-call volume
  - blocked nodes
  - last failure details
- [x] Add explicit run stop semantics to automation runs:
  - `operator_stopped`
  - `guardrail_stopped`
  - `cancelled`
- [x] Surface stop reason and lifecycle history in run detail diagnostics.
- [x] Add explicit `Emergency Stop` labeling in the operator controls.
- [x] Add `Recover Run` action in the run inspector for failed missions.
- [x] Add explicit paused-run recovery so operators can recover from pause with intent, not only resume.
- [x] Expand run inspector diagnostics with:
  - blocked-node reasons
  - failure chain
  - recovery history
  - multi-session transcript visibility
  - per-step activity view linked to node outputs and session transcripts
  - per-step status view with attempts, waiting dependencies, and session/message context
  - per-step log view grouped by node-linked session transcripts
- [x] Add automatic backend guardrails for:
  - token ceilings
  - cost ceilings
  - runtime ceilings
- [x] Surface effective per-node tools and MCP servers in the compile preview.
- [x] Reopen saved `mission_blueprint` automations directly into the advanced builder editor.
- [x] Round-trip saved mission-blueprint metadata back through the advanced builder save flow.
- [x] Harden emergency stop so tracked active sessions and tracked active instances are cancelled and cleared from terminal run state.

## In Progress
- [ ] None

## Remaining Follow-Up
- [~] P0: Tighten the mission kill switch semantics beyond cancel labeling:
  - [x] distinguish `operator_stopped` vs `guardrail_stopped` vs `cancelled` in run state
  - [x] surface stop reason in list view, run detail, and diagnostics/history
  - [x] cancel tracked active sessions and tracked active instances on stop
  - [ ] confirm and harden immediate halt semantics for any remaining untracked session/instance paths
- [ ] P0: Add continue/recovery flow after operator pause, not just failed-run recovery.
  - [x] paused runs resume cleanly
  - [x] operator can recover from pause with explicit intent
  - [ ] downstream state remains coherent under more complex multi-branch scenarios
  - [x] UI clearly shows resumed vs recovered through lifecycle/detail fields
- [ ] P1: Harden advanced automation edit compatibility across older or partially-populated records.
  - [x] unsupported legacy records fall back gracefully
  - [x] older mission-blueprint metadata variants hydrate safely
  - [~] continue hardening reconstruction from sparse compiled automation metadata
- [ ] P1: Add PM semantics to the authored mission/workflow model.
  - [x] define authored `priority`, `phase`, `lane`, and `milestone` semantics
  - keep dependencies as the legality constraint
  - define scheduler behavior:
    - [x] `phase` controls what work is currently open
    - [x] `priority` orders runnable work
    - [x] `lane` supports grouping, visualization, and optional concurrency shaping
    - [x] `milestone` / gates control promotion into later stages
  - [x] decide compile-time vs runtime ownership for phase/milestone behavior
  - [x] update compiler/runtime metadata while keeping `AutomationV2Spec` as the execution target
  - [x] update scheduler selection logic so phase + priority influence execution without violating dependencies
  - [x] expose phase/lane/priority/milestone grouping in the advanced builder UI and compile preview
  - [x] add validation for bad phase references, invalid milestone structure, and illegal barrier sequencing
  - [ ] refine phase-open semantics beyond barrier compilation and runnable ordering
  - [x] add deeper milestone-specific runtime observability and promotion diagnostics
    current state: milestone promotions are now recorded into lifecycle history as structured promotion events and surfaced in the run inspector alongside phase/milestone progress and blocker context
- [ ] P1: Surface full operator diagnostics in the run inspector:
  - [x] gate history
  - [~] per-step logs
    current state: node-linked step logs are visible via grouped session transcripts, status, attempts, waiting-dependency context, and message counts, but a backend-native structured per-step event/log stream is still missing
  - [x] transcripts
  - [x] recovery history
  - [x] blocked-node reasons
  - [x] visible failure chain
- [ ] P2: Add step-level repair flow so an operator can:
  - [x] edit an individual failed step
  - [x] patch the prompt
  - [x] change template/model assignment
  - [x] rerun only the affected subtree
  - [x] keep richer repair history and prompt-diff visibility
- [ ] P2: Add stronger visual dependency graph for workstreams, fan-out, and fan-in in the compile tab.
  - [x] add phase-grouped graph preview in the compile tab
  - [~] continue toward richer visual fan-out/fan-in rendering beyond grouped dependency cards
- [ ] Add richer validation warnings for:
  - [x] unreachable terminal stages not captured by milestone or approval promotion
  - [x] suspicious fan-in/fan-out
  - [x] redundant input refs
  - [x] template/model mismatches
  - [x] align PM-semantics validation with the P1 phase/lane/milestone slice so warnings do not drift from the authored model
- [x] Add more executor regression coverage around mission control/recovery semantics.
  - [x] cover gate rework subtree reset and stale downstream output invalidation
  - [x] cover step repair subtree reset and persisted prompt/template diffs
  - [x] cover paused-run recovery continuity without clearing completed upstream state
  - [x] cover failed-branch recovery preserving completed sibling branches in a multi-branch graph
  - [x] cover paused-run recovery preserving sibling-branch state in a multi-branch graph
  - [x] cover branch-local gate rework preserving completed sibling branches
  - [x] cover branch-local step repair preserving completed sibling branches
  - [x] cover operator stop semantics, stop-kind recording, and active session/instance cleanup
  - [x] move remaining halt-path uncertainty under the P0 kill-switch hardening item instead of leaving it as vague regression work
- [x] Add integration coverage that exercises mission-builder preview/apply through the Tauri boundary.
  - [x] cover `mission_builder_preview` posting `/mission-builder/compile-preview`
  - [x] cover `mission_builder_apply` posting `/mission-builder/apply`
- [ ] Decide whether mission-builder compile logic should remain server-owned or move further into `tandem-workflows`.
- [ ] Improve advanced editing for multiple review/test stages and per-workstream tool/MCP scope controls.
- [ ] Improve the per-step scope editor UX beyond CSV entry:
  - searchable tool selection
  - MCP multi-select from discovered servers
  - clearer inheritance vs override display
- [ ] Add edit/migration support for advanced automations from the automations list, not only new creation flow.

## Explicitly Out Of Scope
- [ ] Replacing `AutomationV2` with a separate parallel runtime for the advanced builder
- [ ] Removing the existing simple automation wizard
- [ ] Making the feature software-development-specific
- [ ] Fully custom runtime semantics outside validated graph compilation

## Risks
- The advanced builder now compiles into the existing `AutomationV2` runtime, which keeps architecture aligned, but some advanced semantics are still represented through metadata and prompt inheritance rather than a deeper dedicated mission runtime.
- Approval gating is implemented as a hard runtime pause point, but operator recovery tooling is still incomplete until failed-step editing, continue flow, and richer gate/rework history are fully exposed.
- Backend guardrails and basic operator stop controls now exist, but the UI still needs sharper distinctions between cancellation, guardrail stop, pause/resume, and repaired recovery paths.
- Without first-class PM semantics, the system can know what is runnable but still fail to express what should go first, what phase the mission is in, or why later work remains intentionally deprioritized or gated.
- The compile preview is structurally useful today, but the graph visualization is still weaker than the product ambition for a power-user swarm mission builder.

## Verification
- [x] `cargo test -p tandem-workflows`
- [x] `cargo check -p tandem-server`
- [x] `cargo test -p tandem-server mission_builder_ -- --test-threads=1`
- [x] `cargo test -p tandem-server automations_v2_run_recover_from_pause_preserves_completed_state_and_records_history -- --test-threads=1`
- [x] `cargo test -p tandem-server automations_v2_run_cancel_records_operator_stop_kind_and_clears_active_ids -- --test-threads=1`
- [x] `cargo test -p tandem-server automations_v2_run_recover_on_failed_branch_preserves_completed_sibling_branch -- --test-threads=1`
- [x] `cargo test -p tandem-server automations_v2_run_recover_from_pause_preserves_branched_state -- --test-threads=1`
- [x] `cargo test -p tandem-server automations_v2_gate_rework_on_failed_branch_preserves_completed_sibling_branch -- --test-threads=1`
- [x] `cargo test -p tandem-server automations_v2_run_repair_preserves_completed_sibling_branch -- --test-threads=1`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml mission_builder_preview_posts_compile_preview_endpoint -- --test-threads=1`
- [x] `cargo test --manifest-path src-tauri/Cargo.toml mission_builder_apply_posts_apply_endpoint -- --test-threads=1`
- [x] `cargo check --manifest-path src-tauri/Cargo.toml`
- [x] `npm run build`

## Notes
- The execution target remains `AutomationV2Spec`, not `MissionSpec`.
- `MissionSpec`/`WorkItem` are currently derived preview/interop artifacts, not the primary runtime contract.
- The simple wizard remains the default low-friction path; the advanced builder is additive.
- The operator model needs to be first-class:
  - stop the mission fast when it is going wrong
  - understand exactly why it failed
  - edit the failing step
  - continue from a repaired state
- The most important shipped semantic change is explicit mission inheritance:
  - mission title
  - mission goal
  - success criteria
  - shared context
  - local assignment
  - dependency context
- Next strategic semantic gap:
  - dependencies already explain legality
  - the model still needs priority, phase, lane, and milestone semantics to explain orchestration intent
