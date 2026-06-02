# Automations V2 Reliability Remediation Kanban

Last updated: 2026-06-02
Owner: Engine / Workflow Runtime
Goal: `docs/internal/v2_repairs/GOAL.md`
Source plan: `docs/internal/v2_repairs/AUTOMATIONS_V2_RELIABILITY_REMEDIATION_PLAN.md`

## Status Legend

- `todo`
- `in_progress`
- `blocked`
- `done`

## Priority Legend

- P0: must land before trusting governed workflow completion.
- P1: core reliability and false-failure reduction.
- P2: lifecycle / feature work or design-gated remediation.

## Now

- `V2R-X3` Terminal checkpoint invariant
  - Status: `done`
  - Priority: P0
  - Scope: prevent inconsistent checkpoint sets from becoming `Completed`.
  - Acceptance: completion fails or repairs when nodes are unaccounted, completed nodes lack outputs, or completed outputs are actually `needs_repair` / `verify_failed` / `failed` / `blocked`.
  - Verification: `cargo test -p tandem-server derive_terminal_run_state -- --nocapture`.

- `V2R-B2` Contract-aware run-level deliverable assertion
  - Status: `done`
  - Priority: P0
  - Scope: implement X1 using a shared deliverable resolver and contract-aware checks.
  - Acceptance: missing / weak required artifacts and required side effects prevent `Completed`; tests cover markdown, JSON, code/write, and external mutation cases.
  - Progress: file deliverable completion gate checks node run artifacts and automation-level output targets for existence and substance; unowned automation-level targets require current-run checkpoint/publication evidence so stale files cannot satisfy completion; high-risk email delivery nodes require receipt-grade success telemetry; generic outbound action nodes require at least one successful recorded `external_actions` receipt before completion.
  - Verification: `cargo test -p tandem-server completion_ -- --nocapture`; `cargo test -p tandem-server derive_terminal_run_state -- --nocapture`; `cargo test -p tandem-server email_delivery -- --nocapture`.

- `V2R-X4` Side-effect receipt assertion
  - Status: `done`
  - Priority: P0
  - Scope: require successful tool-effect / connector receipts for governed external mutations.
  - Acceptance: model prose alone cannot satisfy email / connector delivery requirements.
  - Progress: implemented for the highest-risk governed action in this slice: required email delivery nodes cannot complete from prose alone and require `email_delivery_succeeded` / delivery receipt evidence; missing evidence requeues while attempts remain and fails loudly at the cap.
  - Verification: `cargo test -p tandem-server completion_ -- --nocapture`; `cargo test -p tandem-server email_delivery -- --nocapture`.

## Next

- `V2R-E1` Cycle detection on workflow plan
  - Status: `done`
  - Priority: P0
  - Scope: reject cycles across `depends_on` and `input_refs.from_step_id` in `validate_workflow_plan`.
  - Acceptance: validation returns an authoring-time error naming the cycle.
  - Verification: `cargo test -p tandem-plan-compiler validate_workflow_plan -- --nocapture`.

- `V2R-E5` Normalize `input_refs` readiness dependencies
  - Status: `done`
  - Priority: P0
  - Scope: make compiler and executor agree that `input_refs` sources are readiness dependencies.
  - Acceptance: an input ref cannot execute before its source output exists.
  - Verification: `cargo test -p tandem-plan-compiler validate_workflow_plan -- --nocapture`.

- `V2R-E7` Timer-trigger in-flight dedup
  - Status: `done`
  - Priority: P0
  - Scope: apply `Queued | Running` dedup to scheduled/misfire run creation.
  - Acceptance: slow interval automations do not accumulate queued backlogs for the same automation.
  - Verification: `cargo test -p tandem-server automation_v2_misfires_skip_queued_or_running_runs_for_same_automation -- --nocapture`.

- `V2R-C1` Retry `verify_failed` before terminal failure
  - Status: `done`
  - Priority: P0
  - Scope: remove immediate run-level failure for non-exhausted verification failures.
  - Acceptance: `verify_failed` routes through repair until `max_attempts` is exhausted.
  - Verification: `cargo test -p tandem-server derive_terminal_run_state -- --nocapture`; `cargo test -p tandem-server verify_failed_output -- --nocapture`.

- `V2R-C2-C3` Verification classification and command matching
  - Status: `done`
  - Priority: P0
  - Scope: scope verification markers to verification output and tighten command matching.
  - Acceptance: artifact prose mentioning failed tests does not fail a run; expected commands are matched exactly enough to avoid substring false positives.
  - Verification: `cargo test -p tandem-server verification_command_matching_requires_normalized_command_prefix -- --nocapture`; `cargo test -p tandem-server artifact_prose_about_prior_test_failures_does_not_create_verify_failed_status -- --nocapture`; `cargo test -p tandem-server code_workflow_with_ -- --nocapture`.

- `V2R-E8` Requeue repairable missing deliverables
  - Status: `done`
  - Priority: P0
  - Scope: completion assertion should repair when an owning node still has attempts.
  - Acceptance: missing deliverable requeues owner node with a repair brief instead of immediately failing the run.
  - Verification: `cargo test -p tandem-server completion_deliverable -- --nocapture`.

- `V2R-E9` Unified target resolver
  - Status: `done`
  - Priority: P0
  - Scope: share one resolver across cleanup, prompting, validation, publication, and completion assertion.
  - Acceptance: stale files cannot satisfy completion because cleanup and completion use different target sets.
  - Progress: completion uses the existing automation output path resolver, derives requirements from node output paths plus automation-level output targets, and requires current-run checkpoint/publication evidence for unowned automation-level targets instead of relying on cleanup deletion.
  - Verification: `cargo test -p tandem-server completion_deliverable -- --nocapture`; `cargo test -p tandem-server completion_ -- --nocapture`.

## Backlog

### P1 - Core Reliability

- `V2R-B1` Fix triage-skip fan-in semantics
  - Status: `done`
  - Scope: do not skip a fan-in node when non-triage parents produced real work.
  - Verification: `cargo test -p tandem-server triage_gate -- --nocapture`.

- `V2R-B3-B4-B5` Harden lenient completion tail, validation absence, and empty extraction
  - Status: `done`
  - Scope: make substance and validation presence hard requirements on auto-complete paths.
  - Progress: B5 empty final output without a validated artifact now becomes `needs_repair` and blocks once repair is exhausted. B3/B4 artifact-only completion now requires passing validation metadata; missing, errored, rejected, or unmet validation returns repair/block instead of completion.
  - Verification: `cargo test -p tandem-server artifact_materialized_without_status -- --nocapture`; `cargo test -p tandem-server empty_node_output_without_artifact -- --nocapture`; `cargo test -p tandem-server completion_ -- --nocapture`.

- `V2R-C5` Recoverable tool errors adapt in-turn or retry safely
  - Status: `done`
  - Scope: distinguish recoverable vs fatal tool errors and avoid duplicate side effects on retry.
  - Progress: non-timeout, non-auth tool execution errors now keep failed part/effect telemetry but surface a bounded recoverable tool output to the model instead of aborting the prompt; cancellation and shutdown/runtime-not-ready errors remain prompt-fatal.
  - Verification: `cargo test -p tandem-core nonfatal_tool_execution_error -- --nocapture`; `cargo test -p tandem-core cancellation_and_shutdown_tool_errors_remain_prompt_fatal -- --nocapture`; `cargo test -p tandem-core engine_loop -- --nocapture`.

- `V2R-E2` Preserve partial failure mode into runtime
  - Status: `done`
  - Scope: project `partial_failure_mode` into automation runtime and honor it when blocking descendants.
  - Progress: compiler projected nodes now carry `partial_failure_mode`; server conversion preserves it in node metadata; terminal blocked / verify-failed node outcomes now compute blocked nodes from the runtime mode, with `pause_all` blocking all pending nodes and the default preserving downstream-only blocking.
  - Verification: `cargo test -p tandem-server partial_failure_mode -- --nocapture`; `cargo test -p tandem-server projected_node_metadata_lifts_knowledge_binding -- --nocapture`; `cargo test -p tandem-server automation_v2::executor::tests -- --nocapture`; `cargo test -p tandem-plan-compiler compile_workflow_runtime_projection_shapes_agents_and_nodes -- --nocapture`; `cargo test -p tandem-plan-compiler materialization_seed_roundtrips_projection_shape -- --nocapture`.

- `V2R-E4` Preserve `input_refs` through budget compaction
  - Status: `done`
  - Scope: remap data wiring instead of clearing `input_refs` in compacted plans.
  - Progress: compaction now remaps original input refs to their compacted macro-step source, drops only intra-bucket/unavailable refs, and unions preserved input-ref sources into `depends_on` so validation/readiness remain consistent.
  - Verification: `cargo test -p tandem-plan-compiler generated_research_destination_plan_compacts_to_request_aware_macro_steps -- --nocapture`; `cargo test -p tandem-plan-compiler validate_workflow_plan -- --nocapture`; `cargo test -p tandem-plan-compiler workflow_plan::tests -- --nocapture`.

- `V2R-C4` Broaden required-read satisfaction for synthesized artifacts
  - Status: `done`
  - Scope: let upstream-provided context satisfy read requirements when appropriate.
  - Progress: node status read-gate logic now treats research-synthesis validation with applied upstream evidence or upstream read paths as satisfying requested read requirements, while preserving repair behavior when upstream evidence is absent.
  - Verification: `cargo test -p tandem-server synthesis_upstream_read_evidence_satisfies_required_read_gate -- --nocapture`; `cargo test -p tandem-server required_read_gate_still_repairs_without_upstream_evidence -- --nocapture`; `cargo test -p tandem-server workflow_policy -- --nocapture`.

- `V2R-B7` Validate upstream output quality
  - Status: `done`
  - Scope: block empty or failed upstream output propagation before downstream nodes execute.
  - Progress: upstream input assembly now rejects missing-substance outputs and terminal failure/blocked/repair statuses instead of forwarding empty or failed node output as usable context.
  - Verification: `cargo test -p tandem-server build_upstream_inputs_ -- --nocapture`; `cargo test -p tandem-server normalize_upstream_research_output_paths -- --nocapture`.

- `V2R-B8-E10` Normalize warning pass policy
  - Status: `done`
  - Scope: decide whether `accepted_with_warnings` gates downstream nodes, remains success-with-warning, or becomes non-promotable evidence.
  - Progress: `accepted_with_warnings` remains a passable runtime outcome only when there are no unmet requirements, but it is not treated as a clean validation pass and no longer generates positive workflow-learning memory candidates.
  - Verification: `cargo test -p tandem-server completed_runs_with_validation_warnings_do_not_generate_positive_learning -- --nocapture`.

### P2 - Lifecycle and Design-Gated

- `V2R-X2-A1` Approval-gate lifecycle deadline
  - Status: `done`
  - Scope: add deadline and expiry action for `AwaitingApproval` runs.
  - Progress: approval gates use an explicit manual-only lifecycle policy; overdue `AwaitingApproval` runs are marked with visible stale detail, gate metadata (`stale`, `stale_policy`, `stale_after_ms`), and a lifecycle event, without auto-denying or auto-approving the gate.
  - Verification: `cargo test -p tandem-server awaiting_approval_runs_are_marked_stale_with_visible_manual_policy -- --nocapture`.

- `V2R-D2` Auto-resume `GuardrailStopped` after override approval
  - Status: `done`
  - Scope: resume paused guardrail runs when the matching quota override is approved.
  - Progress: the existing paused-run auto-resume sweep now also resumes `GuardrailStopped` runs when automation governance has an approved, unexpired agent quota override for the automation creator or one of its agents; resumed runs are requeued and clear pause/stop fields with lifecycle evidence.
  - Verification: `cargo test -p tandem-server guardrail_stopped_run_auto_resumes_after_quota_override_approval -- --nocapture`; `cargo test -p tandem-server auto_resume -- --nocapture`.

- `V2R-D1` Runtime capability-grant flow
  - Status: `blocked`
  - Scope: pause missing-but-grantable capability runs and resume after approval.
  - Blocker: runtime-grantable capability list and approval UX; this is a new feature and remains outside the current GOAL non-goal scope.

- `V2R-C7` Idle-based node timeout
  - Status: `done`
  - Scope: replace pure wall-clock timeout with no-progress timeout plus absolute ceiling.
  - Progress: automation node execution now treats the configured timeout as an idle/no-progress budget, resets that budget only on same-session engine progress events, keeps the run-registry heartbeat separate from progress, and enforces a larger absolute ceiling as a hard stop.
  - Verification: `cargo test -p tandem-server automation_node -- --nocapture`; `cargo test -p tandem-server execute_goal_structured_json_default_timeout_uses_long_workflow_budget -- --nocapture`.

- `V2R-C6` Provider idle/connect timeout retry
  - Status: `done`
  - Scope: classify provider idle/connect timeouts as retryable with backoff.
  - Progress: provider connect/idle timeout messages are now classified as transient stream failures; idle timeout failures retry the current provider iteration instead of failing the session immediately; all stream retry branches use a small bounded backoff before retrying.
  - Verification: `cargo test -p tandem-core provider_stream -- --nocapture`.

- `V2R-B6` Iteration-cap exhaustion signal
  - Status: `done`
  - Scope: emit explicit budget-exhausted signal and mark run incomplete distinctly.
  - Progress: prompt execution now tracks the configured iteration budget, fixes iteration numbering when `TANDEM_MAX_TOOL_ITERATIONS` is overridden, and fails the session with `provider.call.iteration.budget_exhausted` instead of appending an idle assistant completion when another model/tool iteration is required but no budget remains.
  - Verification: `cargo test -p tandem-core iteration_budget_exhaustion_fails_run_without_idle_completion -- --nocapture`; `cargo test -p tandem-core engine_loop -- --nocapture`.

- `V2R-D3` Stale-reap tuning
  - Status: `done`
  - Scope: distinguish active work from genuinely wedged provider/tool calls; revisit auto-resume cap.
  - Progress: stale reaping is keyed to provider/session inactivity and honors active run-registry heartbeats, while C7 handles provider idle stalls at node level; the stale auto-resume cap is now configurable via `TANDEM_STALE_AUTO_RESUME_MAX_ATTEMPTS` with the existing default of 2 preserved.
  - Verification: `cargo test -p tandem-server stale_auto_resume -- --nocapture`; `cargo test -p tandem-server stale_running_automation_runs_honor_internal_run_registry_heartbeat -- --nocapture`; `cargo test -p tandem-server stale_running_automation_runs_fail_terminal_in_progress_nodes -- --nocapture`.

- `V2R-C8` Cross-restart in-flight run resume
  - Status: `blocked`
  - Scope: checkpoint/resume live engine sessions across server restart.
  - Blocker: explicit scope decision; likely requires new session-state persistence; this remains outside the current GOAL non-goal scope.

### Defensive / Policy Items

- `V2R-E3` Planner fallback should not silently reuse stale plans
  - Status: `done`
  - Priority: P1
  - Scope: distinguish failed revisions from no-op revisions and block activation on failure.
  - Progress: failed planner revisions now return clarifier metadata with `revision_failed`, `blocks_activation`, and `failure_reason`; draft revision numbers only advance for successful revisions or intentional keep/clarify outcomes, not invalid planner responses or provider failures.
  - Verification: `cargo test -p tandem-plan-compiler failed_planner_revision_does_not_advance_plan_revision -- --nocapture`; `cargo test -p tandem-plan-compiler planner_drafts::tests -- --nocapture`; `cargo test -p tandem-plan-compiler curated_api_supports_json_build_and_revision_flow -- --nocapture`.

- `V2R-E6` StrictSequential dependency handling
  - Status: `done`
  - Priority: P1
  - Scope: topologically order or reject declared order that contradicts dependencies.
  - Progress: `StrictSequential` now rejects a declared step order that places a step before an internal dependency, and plan validation reports a blocking `strict_sequential_order_conflict` issue with `dependencies_resolvable = false`.
  - Verification: `cargo test -p tandem-plan-compiler strict_sequential -- --nocapture`; `cargo test -p tandem-plan-compiler dependency_planner -- --nocapture`; `cargo test -p tandem-plan-compiler plan_validation -- --nocapture`.

- `V2R-A2` Rate-limit permanent-throttle footgun
  - Status: `done`
  - Priority: P1
  - Scope: treat missing throttle expiry as not throttled or reject it at construction.
  - Progress: provider throttles without an expiry no longer block admission forever; expired throttles also report inactive while active future expiries still throttle.
  - Verification: `cargo test -p tandem-server rate_limit::tests -- --nocapture`.

- `V2R-C9` Tool-guard budget default alignment
  - Status: `done`
  - Priority: P1
  - Scope: align code and `.env.example`; document email-specific cap.
  - Progress: `.env.example` now matches the code default by enforcing per-run tool guard budgets (`TANDEM_DISABLE_TOOL_GUARD_BUDGETS=0`) and documents that email delivery remains capped at 1 unless explicitly overridden.
  - Verification: `cargo test -p tandem-core tool_budget -- --nocapture`.

- `V2R-C10` Permission timeout / auto-approve-denied behavior
  - Status: `done`
  - Priority: P1
  - Scope: make required side-effect permission failures loud in automation/headless contexts.
  - Progress: write-required/headless tool calls now return prompt-fatal errors for permission auto-approve denial, user denial, and permission wait timeout instead of surfacing a normal model-consumable tool-output string.
  - Verification: `cargo test -p tandem-core write_required_ -- --nocapture`.

- `V2R-D4` OSS governance fail-closed documentation
  - Status: `done`
  - Priority: P2
  - Scope: no code fix; intended licensing behavior.

## Done

- `V2R-DOC-001` Create remediation plan
  - Status: `done`
  - Scope: `AUTOMATIONS_V2_RELIABILITY_REMEDIATION_PLAN.md` captures architecture, phases, design decisions, and estimates.

- `V2R-DOC-002` Create workstream goal
  - Status: `done`
  - Scope: `GOAL.md` defines objective, success criteria, non-goals, completion gates, and verification plan.

- `V2R-DOC-003` Create kanban
  - Status: `done`
  - Scope: `KANBAN.md` maps remediation cards into now / next / backlog / blocked / done lanes.

## Risks / Watchouts

- Completion assertions must not make repairable failures terminal too early.
- File deliverables and external side effects need different evidence models.
- `output_targets`, node required paths, builder metadata, and publish specs must not diverge.
- Verification fixes can expose currently hidden workflow failures; expect test churn.
- Design-gated cards should stay blocked until decisions are explicit.

## Verification Commands

Run the narrowest relevant checks for each slice first, then broaden:

- `cargo test -p tandem-plan-compiler`
- `cargo test -p tandem-server automation_v2`
- `cargo test -p tandem-server automation::`
- `cargo test -p tandem-core engine_loop`

Adjust exact filters to the touched modules and current crate test names.
