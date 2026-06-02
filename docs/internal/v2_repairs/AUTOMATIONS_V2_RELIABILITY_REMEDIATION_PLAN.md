# Automations V2 Reliability Remediation Plan

Subject: Closing the gaps where governed workflows fail or get blocked without completing their assigned task.

Codebase: `frumu-ai/tandem`

Branch baseline: `main @ 0.5.13` after the security-hardening merge

Status: Engineering plan derived from static analysis. Effort figures are judgment estimates for one engineer familiar with the codebase; add roughly 15-20% for review, integration testing, and regressions surfaced by the large automation test suites.

## How to read this document

Each remediation card has:

- ID / Severity: severity is likelihood x impact (`Critical`, `High`, `Medium`, `Low`).
- Failure mode: what goes wrong for the workflow today.
- Root cause (code): `file:line` where the behavior lives.
- Change: what to do.
- Touch points: files to modify and the test files most likely to need updating.
- Effort: engineering-days.
- `[DECISION]`: blocked on a product/design decision, noted in [Open design decisions](#open-design-decisions).

All paths are relative to the repo root unless noted. Line numbers reflect the analyzed tree and will drift as the code changes; treat them as anchors, not exact addresses.

## 1. Executive framing

A run's terminal state is decided almost entirely by `derive_terminal_run_state` (`crates/tandem-server/src/automation_v2/executor.rs:993-1001`): a run is `Completed` iff no node is pending, failed, or blocked. There is no run-level check that the declared `output_targets` / deliverables actually exist. That single fact is the root of the most dangerous failure class, silent false-completion, and is the highest-leverage thing to fix.

The failure modes split into five families:

| Family | Theme | Visibility |
| --- | --- | --- |
| A | Permanent hangs, never terminal | Silent |
| B | False-completion, done without the work | Silent |
| C | False-failure, legit work killed and often not retried | Loud |
| D | Blocked states needing manual recovery | Loud |
| E | Structural / DAG / planning gaps | Mixed |

The cross-cutting fixes in section 2 collapse a large share of B and A in one place each. The most important correction to the original plan is that "output target exists" is not a sufficient completion invariant. The runtime must also prove that every node is accounted for and that every required file, external side effect, or explicit skip has a valid terminal receipt.

## 2. Cross-cutting architectural fixes

Do these first.

### X1 - Run-level deliverable completion assertion

Eliminates the silent variants of B1, B2, B3, B5, and B6.

Effort: 2-3d, counted under B2 below; do not double-count.

Before a run is allowed to transition to `Completed`, assert that every declared deliverable for terminal / deliverable nodes is satisfied. A deliverable can be a run artifact, required workspace file, published durable file, or external side-effect receipt. Today completion is primarily a node-count check.

Do not put filesystem-heavy logic directly into `derive_terminal_run_state`; that helper is currently pure. Add the gate in the completion transition paths around `crates/tandem-server/src/automation_v2/executor.rs:1157-1164` and `:1296-1302`, or pass a precomputed completion assertion into terminal derivation.

Reuse the existing substance helper where it fits: `structural_substantive_artifact_text` at `crates/tandem-server/src/app/state/automation/logic_parts/part03.rs:960`. Make checks contract-aware:

- Markdown / report artifacts must pass structural substance.
- JSON artifacts must parse and satisfy the expected schema / required fields.
- Code tasks must satisfy required writes and verification status.
- External mutations must have tool-effect / connector receipts, not just model text.

The deliverable source of truth should be one resolver that includes automation-level `output_targets`, per-node required output paths, builder-declared `output_files` / `must_write_files`, publish specs, and external side-effect requirements. Keep this resolver shared between cleanup, prompting, validation, and completion assertion. Current cleanup uses `automation_declared_output_paths_for_run` around `crates/tandem-server/src/app/state/automation/logic_parts/part05.rs:1158-1178`; do not let cleanup and completion drift.

### X2 - Gate / pause lifecycle with deadlines and auto-resume

Eliminates A1 and D2.

Effort: counted under A1 and D2.

Introduce a single sweep that handles non-`Running` parked states the current reaper ignores:

- Expire or escalate `AwaitingApproval` (A1).
- Auto-resume `GuardrailStopped` pauses once their override is approved (D2).

Current reaper, which only handles `Running`: `crates/tandem-server/src/app/state/app_state_impl_parts/part03.rs:1420-1438`.

Auto-resume, which only handles `StaleReaped`: `crates/tandem-server/src/app/state/app_state_impl_parts/part05.rs:251-291`.

### X3 - Terminal checkpoint invariant

Eliminates false-completion caused by inconsistent checkpoint sets.

Effort: 1-2d, additive to B2 unless implemented in the same assertion layer.

Before a run can become `Completed`, assert that:

- Every flow node is represented by exactly one valid terminal accounting state: completed, blocked, failed, pending, or explicitly skipped.
- Every `completed_nodes` entry has a corresponding `node_outputs` entry.
- No output with `status = needs_repair`, `verify_failed`, `failed`, or `blocked` is counted as completed.
- `pending_nodes`, `completed_nodes`, and `blocked_nodes` are mutually consistent after dedupe.
- Skipped nodes carry explicit skip metadata and do not satisfy required deliverables unless the skip reason is allowed by policy.

Touch points: `crates/tandem-server/src/automation_v2/executor.rs`, `executor_tests.rs`, and run-state tests in `crates/tandem-server/src/app/state/automation/tests_parts/*`.

### X4 - Run-level side-effect receipt assertion

Closes the gap where governed side effects are skipped but the node still produces text.

Effort: 1-2d for the initial receipt gate; more if connector-specific receipts are missing.

For nodes that require external mutations such as email, Notion, GitHub, Slack, browser actions, or other connector writes, completion must require a successful tool-effect / connector receipt. Model prose like "sent" or "created" is not evidence. This is the non-file companion to X1.

Touch points: tool telemetry in `crates/tandem-core/src/engine_loop.rs`, node output classification in `node_output_parts/part02.rs`, and automation attempt evidence / receipt plumbing in `logic_parts/part04.rs` and `part05.rs`.

## 3. Phase 1 - Quick wins

Approximate effort: 8-12 days, low risk.

### E1 - Cycle detection on the workflow plan

Severity: High

Effort: 1d

Failure: `A -> B`, `B -> A` passes validation, compiles, runs, and neither node ever becomes runnable, then eventually fails late with generic `Failed{"flow deadlock"}`. The same problem can hide in data dependencies if `input_refs` create a cycle or point to a node not represented in `depends_on`.

Root cause: `crates/tandem-plan-compiler/src/workflow_plan_parts/part01.rs:2017-2035` rejects dangling deps but does no cycle check. Cycle detection exists only for the `PlanPackage` routine graph at `crates/tandem-plan-compiler/src/dependency_planner.rs:142-148`, not for `flow.nodes`.

Change: Add a topological-sort / DFS back-edge check over the union of `plan.steps[].depends_on` and `plan.steps[].input_refs[].from_step_id` in `validate_workflow_plan`; return a clear authoring-time error naming the cycle.

Touch points: `workflow_plan_parts/part01.rs`; tests in `crates/tandem-plan-compiler/tests/`, for example `api_surface.rs` and `contracts_roundtrip.rs`.

### E3 - Planner fallback silently keeps old / partial plan

Severity: Medium

Effort: 1d

Failure: On unconfigured model, provider failure, or invalid response, the revise loop returns the prior plan with a clarifier; a user's revision is silently discarded and an incomplete DAG runs.

Root cause: `crates/tandem-plan-compiler/src/planner_loop.rs:90-124`, `:157-191` returns `current_plan.clone()`.

Change: Distinguish "revision produced no change" from "revision failed". On failure, surface an error / explicit `needs_clarification` state that blocks activation rather than silently reusing the old plan.

Touch points: `planner_loop.rs`; planner tests.

### E5 - input_refs vs depends_on readiness mismatch

Severity: Low-Medium

Effort: 1d

Failure: Runnability is gated only on `depends_on` (`executor.rs:1222`), but input assembly reads `input_refs` (`upstream.rs:119-150`). An `input_ref` to a node not also in `depends_on` is never waited for and `bail!`s at execution.

Change: Prefer compile-time normalization: every `input_ref.from_step_id` should be added to `depends_on` or rejected if that would create a cycle. Also make the executor readiness set use the same normalized dependency view so compiler and runtime cannot disagree.

Touch points: `crates/tandem-server/src/automation_v2/executor.rs:1222`; `crates/tandem-server/src/app/state/automation/upstream.rs:119-150`; validation in `workflow_plan_parts/part01.rs`.

### E6 - StrictSequential bypasses the dependency graph

Severity: Medium

Effort: 0.5-1d

Failure: Steps are emitted in declared order ignoring `depends_on`; topology guard cannot trip because `planned` is set to all ids unconditionally.

Root cause: `crates/tandem-plan-compiler/src/dependency_planner.rs:91-98`.

Change: Either topologically order within `StrictSequential`, or reject a declared order that contradicts `depends_on` with a diagnostic.

### E7 - Timer triggers have no in-flight dedup

Severity: Medium

Effort: 0.5-1d

Failure: Scheduled automations slower than their interval accumulate a backlog of `Queued` runs.

Root cause: watch path dedups (`crates/tandem-server/src/app/state/app_state_impl_parts/part04.rs:112-130`); timer / misfire path (`part04.rs:55-82`, consumed at `crates/tandem-server/src/app/tasks.rs:1233-1252`) does not.

Change: Apply the same `Queued | Running` dedup to the timer path before creating a run.

### A2 - Rate-limit permanent-throttle footgun

Severity: Low

Effort: 0.5d

Root cause: `crates/tandem-server/src/app/state/automation/rate_limit.rs:29-39`: `is_throttled && throttled_until_ms == None` returns true forever.

Change: Treat `None` expiry as "not throttled", or require an expiry at construction. Defensive; current writer is safe.

### E8 - Completion assertion should requeue repairable missing deliverables

Severity: High

Effort: 1-2d

Failure: Once the final completion assertion exists, a missing or non-substantive deliverable could fail the run even when the responsible node still has attempt budget. That would convert a silent false-completion into a loud but unnecessarily terminal failure.

Root cause: today there is no run-level deliverable assertion. When it is added, it needs to feed the same repair queue as node-level validation instead of acting only as a final hard fail.

Change: If the completion assertion finds a missing / invalid deliverable and the owning node has attempts remaining, put that node back in `pending_nodes` with a repair brief. Only fail or block when the owning node is exhausted or no owner can be resolved.

Touch points: `executor.rs` completion transition paths; `render_automation_repair_brief`; run-state tests.

### E9 - Unify declared output cleanup and completion target resolution

Severity: Medium

Effort: 0.5-1d

Failure: Run-start cleanup and run-end completion checks can drift if they use different target lists. That allows stale files to satisfy a completion check or valid run-scoped artifacts to be removed unexpectedly.

Root cause: cleanup currently uses `automation_declared_output_paths_for_run` (`logic_parts/part05.rs:1158-1178`) while prompting / validation / publication also consider per-node metadata and publish specs.

Change: Introduce one normalized resolver for required run artifacts, durable workspace writes, and external side-effect expectations. Use it for cleanup, prompt rendering, validation, and X1/X4 completion assertions.

Touch points: `logic_parts/part05.rs`, `logic_parts/part02.rs`, `workflow_impl.rs`, completion assertion tests.

### E10 - Normalize warning policy across executor, learning, and downstream gates

Severity: Medium

Decision-gated: yes

Effort: 1d

Failure: `accepted_with_warnings` is treated as passing in some paths and merely surfaced in others. Downstream nodes and workflow-learning metrics can learn from or depend on outputs that had warnings without a consistent policy decision.

Root cause: `automation_output_is_passing` treats `accepted_with_warnings` as passing (`logic_parts/part05.rs:1084-1092`); workflow learning only marks `failed | blocked` as validation failures (`workflow_learning.rs:370-383`).

Change: Decide whether warnings are success-with-warning, a soft gate, or non-promotable evidence. Encode that once and use it for downstream readiness, learning, and lifecycle reporting.

Decision needed: warning policy.

### C9 - Tool-guard budget default mismatch

Severity: Medium

Decision-gated: yes

Effort: 0.5-1d

Failure: `TANDEM_DISABLE_TOOL_GUARD_BUDGETS` defaults to false, meaning enforced, in code when unset, but `.env.example` ships `=1`. Email delivery remains capped at 1 even when general budgets are disabled unless `TANDEM_TOOL_BUDGET_EMAIL_DELIVERY` is set.

Root cause: `crates/tandem-core/src/engine_loop/loop_guards.rs:81-91` default; email-specific limit at `:5-17`; `.env.example:16` ships the disabled setting.

Change: Decide the intended default and make code and `.env.example` agree; document that the email cap is separate from general tool-budget disabling.

Decision needed: which default is correct.

### C10 - Permission-wait timeout in unattended runs

Severity: Medium

Decision-gated: yes

Effort: 1d

Failure: In sessions without automation auto-approval, no human reply within 15s means the tool returns a timeout string, the model can continue, and the governed side effect may never happen. In normal V2 node execution, auto-approval is enabled, but auto-approval can still deny shell or non-allowlisted tools immediately, creating a similar "side effect skipped" failure class.

Root cause: permission wait timeout in `crates/tandem-core/src/engine_loop.rs:780-835`; default `crates/tandem-core/src/engine_loop/loop_tuning.rs:49-55`; V2 auto-approval setup in `crates/tandem-server/src/app/state/automation/logic_parts/part05.rs:2029`; auto-approval deny branch in `engine_loop.rs:680-719`.

Change: Define headless / automation behavior separately for true permission timeout and auto-approve-denied cases. For required side effects, prefer fail / block the node loudly instead of returning a normal tool-output string that lets the model continue.

Decision needed: fail/block vs skip-and-continue for required side effects.

## 4. Phase 2 - Core reliability

Approximate effort: 23-36 days, test-heavy.

### B2 - Run-level deliverable assertion

Severity: High

Effort: 2-3d

Implements X1 and should usually be implemented with X3/X4. Highest-leverage single change.

Touch points: `executor.rs:1157-1164`, `:1296-1302`; deliverable resolver / cleanup in `logic_parts/part05.rs:1139-1178`; substance helper `logic_parts/part03.rs:960`; node-output validation in `node_output_parts/part02.rs`; run-state tests in `crates/tandem-server/src/app/state/automation/tests_parts/*` and any `automation_v2` executor tests.

### B1 - Triage-skip propagates to fan-in nodes with real-work parents

Severity: High

Effort: 2-3d

Failure: A node depends on `[triage_gate, heavy_research]` and is skipped when the triage gate says "no work", even though `heavy_research` produced output. The run completes with no deliverable.

Root cause: `crates/tandem-server/src/automation_v2/executor.rs:835-872` (`should_skip_due_to_triage_gate`) returns skip if any triage parent has no work; skipped nodes pushed to `completed_nodes` at `:1243-1254`.

Change: Only skip when all non-triage parents also yield no work, or when the node's role is purely triage-dependent. Otherwise run the node.

Touch points: `executor.rs:835-872`; executor tests; interacts with B2 as a backstop.

### B3 + B4 + B5 - Lenient completion tail / optional validation / empty to success

Severity: High

Effort: 4-6d

B3 root cause: `crates/tandem-server/src/app/state/automation/node_output_parts/part02.rs:754`: `artifact_materialized && !status_signal_present` becomes `completed`. Validation does assess candidate content elsewhere, but this lenient tail means any upstream path that incorrectly yields `verified_output` becomes a silent completion.

B4 root cause: status branches read `artifact_validation.and_then(...)`; when validation is `None`, all blocks bypass to the lenient tail (`part02.rs:754` / `:774`).

B5 root cause: extraction returns raw / empty (`crates/tandem-server/src/app/state/automation/extraction.rs:50-66`, `:150-170`), wrapped as "completed successfully" (`part02.rs:1440-1443`).

Change:

- Make substance a hard requirement on the auto-complete path.
- Treat absent / errored validation as a loud non-terminal condition, not success.
- Flag empty extraction as failure, not completion.

Touch points: `node_output_parts/part02.rs`, currently 1,697 lines and likely to cause heavy test churn; `extraction.rs`; tests in `tests_parts/*` and `node_output_parts/*`.

### C1 - verify_failed is terminal with zero retries

Severity: High

Effort: 2-3d

Root cause: `crates/tandem-server/src/automation_v2/executor.rs:1976-1988` sets the whole run to `Failed` immediately after a node output is classified as `verify_failed`; repair re-queue at `:894-917` excludes `verify_failed`; node dropped from pending at `:1781`.

Change: Remove the immediate run-failure branch for non-exhausted `verify_failed` attempts. Route `verify_failed` through the repair path so it retries within `max_attempts`; only block descendants and mark the run terminal after verification retries are exhausted.

Touch points: `executor.rs` repair / terminal logic; executor tests.

### C2 + C3 - Verification classification and command matching

Severity: High / Medium

Effort: 3-4d

C2 root cause: `crates/tandem-server/src/app/state/automation/verification.rs:253` fails on any nonzero exit; prose markers at `node_output_parts/part02.rs:423-431` scan the artifact / session body, so a report mentioning "tests failed" fails the run.

C3 root cause: `verification.rs:281` matches the expected command as a loose lowercase substring of any executed command; compound `&&` / `;` split at `:74-96` yields partial, then blocked for code workflows (`part02.rs:548-560`).

Change:

- Allow benign nonzero exits / make the success criterion explicit per node.
- Scope the marker scan to verification command output only, not the artifact body.
- Tighten command matching to a normalized exact / whole-token match.

Touch points: `verification.rs`, `node_output_parts/part02.rs`; verification tests.

### C5 - Recoverable tool errors abort the engine turn instead of allowing adaptation

Severity: Medium-High

Effort: 2-3d

Failure: A non-timeout tool error aborts the current engine turn. In V2, the executor catches this as a node execution error and retries until the node attempt budget is exhausted, but the model loses the chance to adapt within the same turn. For mutating workflows this can also increase duplicate side-effect risk on retry.

Root cause: `crates/tandem-core/src/engine_loop.rs:1308-1335` does `return Err(err)` for any tool error that is not a tool-exec timeout or MCP-auth error; propagates at `crates/tandem-core/src/engine_loop/prompt_execution.rs:1265-1271` into `mark_session_run_failed`. V2 then records/retries the node in `crates/tandem-server/src/automation_v2/executor.rs:1990-2265`.

Change: Surface recoverable tool errors to the model as tool output, like the timeout branch already does, so it can adapt, while keeping genuinely fatal errors fatal. Define the fatal vs recoverable taxonomy and make sure retries cannot duplicate external mutations without receipts / idempotency keys.

Touch points: `engine_loop.rs:1227-1335`; model the new branch on the existing timeout branch at `:1227-1259`; V2 retry handling in `executor.rs:1990-2265`; engine-loop tests in `crates/tandem-core/src/engine_loop/tests/*`.

### E2 - Compiled PartialFailureMode dropped at runtime

Severity: High

Effort: 2-3d

Root cause: defined at `crates/tandem-plan-compiler/src/dependency_planner.rs:21`, but `ProjectedAutomationNode` does not carry it (`crates/tandem-plan-compiler/src/automation_projection.rs:45-65`) and `runtime_projection.rs:47-64` does not project it. The executor then unconditionally blocks all transitive descendants at `crates/tandem-server/src/automation_v2/executor.rs:1764-1795` via `collect_automation_descendants`.

Change: Thread the compiled per-node `partial_failure_mode` into the projection schema / generated automation metadata and have the executor honor "continue on partial failure" instead of always blocking descendants.

Touch points: `automation_projection.rs`, `runtime_projection.rs`, materialization / projection tests, `executor.rs:1764-1795`, and executor tests.

### E4 - Budget compaction drops all input_refs

Severity: Medium

Effort: 1-2d

Root cause: `crates/tandem-plan-compiler/src/workflow_plan_parts/part01.rs:868-936` re-buckets plans larger than 8 steps into a hardcoded keyword scheme and sets `step.input_refs = Vec::new()` at `:936`.

Change: Preserve / remap data wiring through compaction instead of clearing it; avoid collapsing non-matching steps into generic `synthesize_work` when they carry real dependencies.

Touch points: `workflow_plan_parts/part01.rs`; compaction tests.

### C4 - Required-read gate rejects valid synthesized artifacts

Severity: Medium

Effort: 1-2d

Root cause: `crates/tandem-server/src/app/state/automation/node_output_parts/part02.rs:589-617` blocks nodes where `requested_has_read && !executed_has_read`.

Change: Treat upstream-provided context as satisfying the read requirement; broaden `skip_read_gate_because_explicitly_completed` / the `research_synthesis` profile.

### B7 + B8 - Empty upstream propagation / accepted_with_warnings as pass

Severity: Medium

Effort: 2-3d

B7 root cause: `crates/tandem-server/src/app/state/automation/upstream.rs:126-141` forwards an empty / garbage output when the key is present. Missing keys already `bail!`.

B8 root cause: `logic_parts/part05.rs:1084` and `workflow_learning.rs:370-383` treat `accepted_with_warnings` as passing.

Change: Add an emptiness / quality check before propagating upstream output; decide whether warnings should gate downstream, likely surface but not silently pass.

Decision needed for B8: warning policy.

## 5. Phase 3 - Features and design-gated work

Approximate effort: 19-33 days.

### A1 - Approval-gate lifecycle

Severity: High

Decision-gated: yes

Effort: 3-4d

Implements X2, part 1.

Failure: `AwaitingApproval` runs never expire or escalate; they hang forever across restarts. Current workspace locking does not appear to lock `AwaitingApproval` (`automation_status_holds_workspace_lock` only locks `Running | Pausing`), so the primary risk is an indefinite non-terminal state and stale operator UX rather than workspace starvation.

Root cause: `crates/tandem-server/src/app/state/automation/gates.rs:15-24` parks the run; gate carries `requested_at_ms` (`logic_parts/part01.rs:1349-1375`) but nothing reads it; reaper ignores non-`Running` (`app_state_impl_parts/part03.rs:1420-1438`); restart re-registers but does not time out (`part05.rs:220`).

Change: Add a configurable gate deadline and a sweep that, on expiry, takes the chosen action: auto-deny, escalate to another surface, or notify-and-extend.

Decision needed: what expiry does.

Touch points: `gates.rs`, gate struct in `automation/types.rs:1029`, new sweep in `app_state_impl_parts/part03.rs`; gate decision endpoint plumbing.

### D2 - Auto-resume on guardrail-override approval

Severity: Medium-High

Effort: 1-2d

Implements X2, part 2.

Root cause: `auto_resume_stale_reaped_runs` only handles `StaleReaped` (`app_state_impl_parts/part05.rs:251-291`); spend / budget pauses are `GuardrailStopped` (`app/state/governance.rs:1712-1754`; `governance-engine/lib.rs:848-892`; budget at `logic_parts/part01.rs:14-45`, `executor.rs:1112-1144`).

Change: Extend auto-resume to `GuardrailStopped`, triggered when the corresponding `QuotaOverride` approval is granted.

### D1 - Runtime capability-grant flow

Severity: Medium-High

Decision-gated: yes

Effort: 5-8d

Failure: A node whose capability is unavailable / not offered currently receives a `needs_repair` capability-resolution output and is retried until attempt budget is exhausted. After that it becomes terminal blocked/failed without a runtime "request grant -> pause -> human approves -> resume" path.

Root cause: `crates/tandem-server/src/app/state/automation/capability_impl.rs:253-292`; error output at `logic_parts/part05.rs:1888-1955`; non-terminal execution error output at `executor.rs:375-434`; terminal derivation at `executor.rs:954-991`. Governance escalation today governs authoring, not per-run provisioning (`crates/tandem-governance-engine/src/lib.rs`).

Change: New runtime flow that pauses the run on a missing-but-grantable capability and resumes on approval. This is a feature, not a fix.

Decision needed: which capabilities are runtime-grantable and which UX surface owns approval.

### C7 - Idle-based node timeout

Severity: Medium-High

Effort: 2-3d

Root cause: `crates/tandem-server/src/app/state/automation/logic_parts/part05.rs:1356-1376` is a total wall-clock cap. Defaults at `:1399-1414`: 600s generic, 180s structured, 120s standup; long-execute heuristic at `:1417`; env clamp `config/env.rs:50-57`.

Change: Convert to a no-progress / idle timeout, reset on heartbeat / streaming activity, so legitimate long work is not aborted; keep an absolute ceiling.

### C6 - Provider idle/connect timeout retry

Severity: Medium-High

Effort: 1-2d, risky

Root cause: `crates/tandem-core/src/engine_loop/prompt_execution.rs:640-665`, `:725-750`; idle-timeout string does not match transient patterns in `prompt_helpers.rs:113-117`, so it is never retried; retries `completion.clear()` and drop partial text (`:636`).

Change: Classify idle / connect timeouts as retryable with backoff; ideally resume rather than restart the iteration. Provider streams generally are not resumable, so this may be "retry the call" only.

### B6 - Iteration-cap exhaustion signal

Severity: High, silent at engine

Effort: 1-2d

Root cause: `crates/tandem-core/src/engine_loop/prompt_execution.rs:169`, `:222-224`; default 25 in `loop_tuning.rs:1-8`; loop exits to completion synthesis and the session idles as success.

Change: Emit an explicit "iteration budget exhausted" signal and mark the run incomplete distinctly, so it is not reported as success and downstream does not have to infer it from a missing `status:"completed"`. Also fix the cosmetic hardcoded 26 in the iteration display when the cap is overridden.

### D3 - Stale-reap tuning

Severity: Medium

Effort: 1-2d

Root cause: `crates/tandem-server/src/app/state/automation/tasks.rs:13` (600s); reaper `app_state_impl_parts/part03.rs:1420-1603`; resume capped at 2 in 20 min (`part05.rs:251-289`, window at `part05.rs:1`).

Change: Detect genuinely hung tool / MCP calls more precisely so active work is not reaped; revisit the 2-resume cap.

### C8 - Resume in-flight runs after engine restart

Severity: Medium-High

Decision-gated: yes

Effort: 5-10d, possibly "won't do"

Root cause: `crates/tandem-server/src/app/state/app_state_impl_parts/part05.rs:158`: `recover_in_flight_runs` converts `Running` to `Failed(ServerRestart)`; only `Pausing -> Paused` and lock re-reservation are handled.

Change: Checkpoint / resume live engine sessions across restart instead of failing them. This is the largest item and likely requires session-state persistence not present today.

Decision needed: whether this is in scope at all.

### D4 - OSS governance fail-closed

Severity: Low-Medium

Effort: no fix, intended

`crates/tandem-server/src/app/state/governance.rs:17-66` returns `feature_unavailable` for agent-initiated create / escalate / mutate in OSS builds. Documented as intended licensing behavior.

## 6. Open design decisions

- A1: On approval-gate expiry, do we auto-deny, escalate to a second surface, or notify-and-extend? What is the default deadline?
- C9: Is the intended default for tool-guard budgets enforced, as code does today, or disabled, as `.env.example` implies?
- C10: In unattended / automation runs, should a permission-wait timeout fail the node loudly or skip-and-continue as today?
- B8/E10: Should `accepted_with_warnings` gate downstream nodes, remain success-with-warning, or become non-promotable evidence for learning?
- D1: Which capabilities are runtime-grantable, and through which approval surface?
- C8: Is cross-restart resume of in-flight runs in scope, or is "fail loudly plus manual re-trigger" acceptable?

## 7. Effort summary

| Phase | Scope | Effort |
| --- | --- | --- |
| 1 - Quick wins | E1, E3, E5, E6, E7, A2, E8, E9, E10, C9, C10 | 8-12 eng-days |
| 2 - Core reliability | B2 (= X1/X3/X4), B1, B3/B4/B5, C1, C2/C3, C5, E2, E4, C4, B7/B8 | 23-36 eng-days |
| 3 - Features / design-gated | A1 (= X2a), D2 (= X2b), D1, C7, C6, B6, D3, C8 | 19-33 eng-days |
| Total |  | 50-81 eng-days, roughly 10-16 weeks solo or 5-8 weeks with two engineers |

Add roughly 15-20% for review, integration testing, and regressions from the large `tests_parts/*` and `node_output_parts/*` suites.

Recommended minimum-viable slice, roughly 2.5-3.5 weeks: X3 checkpoint invariant, X1/B2 contract-aware deliverable assertion, X4 side-effect receipt assertion for the highest-risk governed actions, E1/E5 graph validation using `depends_on + input_refs`, E7 timer dedup, and C1/C2/C3 verification retry/classification fixes. This removes most silent false-completions and most false-failures without taking on the large features D1, C8, and the full A1 lifecycle.

## 8. Suggested sequencing

Week 1:

- Start with X3, then X1/B2 and X4 so completion has a real contract before broad graph/runtime changes land. In parallel, take E1/E5 and E7 because they are low-risk compiler/scheduler hygiene.

Week 2:

- Finish B2 deliverable assertion and E8 repairable requeue behavior.
- C1 verification retry path.
- Start C2/C3 verification classification and command matching.

Week 3:

- Finish C2/C3.
- Add regression tests around false-completion, missing side-effect receipts, inconsistent checkpoint sets, and false-failure cases.
- Re-run the larger automation suites and classify any newly surfaced failures as real behavior gaps vs test expectation updates.

After the minimum-viable slice:

- Proceed into B1 and B3/B4/B5 if silent completion remains the priority.
- Proceed into A1/D2 if parked-run recovery is the priority.
- Defer D1 and C8 until product/design decisions are made.
