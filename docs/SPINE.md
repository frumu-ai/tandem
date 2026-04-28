# Automation Spine

Three invariants that the automation/workflow system depends on. They are
violated often enough — by drift, by silent additions of new tool names, by
the run/UI projection diverging — that the same shape of bug keeps coming
back in different files. This document names them, says where they live in
code, and lists what each phase pins down.

The goal is **one source of truth per invariant** so the compiler (or a
property test) prevents a future change from reintroducing the bug class.

## Invariant 1 — Write-target derivation

> For any tool call, the set of paths it writes to is a pure function of
> `(tool name, args)`. For every read-only tool, this set is empty.

**Owns this:** `crates/tandem-core/src/engine_loop/write_targets.rs`

**Why it matters:** The write-target set drives session write policy,
preflight gating, and artifact-output detection. If a read tool is ever
classified as a write (commit `0c6e7dd`), the engine will incorrectly
flag source-file writes; if a new write tool is added but not classified,
its writes slip past the gate.

**Enforcement:** A `ToolKind` enum + exhaustive `match` in `write_targets`
and `requires_concrete_write_target`. Adding a new tool variant fails to
compile until both functions have an arm for it. A property test asserts
that every `ToolKind` variant marked read-only returns `∅` for any args.

**Filled by:** Phase 1.

## Invariant 2 — MCP readiness before tool dispatch

> Every concrete tool call reaches a connected MCP server, or it fails fast
> with a typed error. No call may land on a stale or partially reconnected
> connection.

**Owns this:** `crates/tandem-runtime/src/mcp_ready.rs`

**Why it matters:** Recent fixes (`852c453`, `f6bf753`, `e88e951`) added
reconnect logic at multiple call sites. Each site must independently get
this right; one missed site is a stuck or panicking run.

**Enforcement:** A single `ensure_mcp_ready(tool) -> Result<Conn,
McpReadyError>` gate. The raw connection type becomes private to the
runtime crate; only the gate hands one out. Callers cannot acquire a
connection without going through the readiness check, so the compiler
rejects any new caller that tries.

**Filled by:** Phase 2.

## Invariant 3 — Run/task mutability is a single derived predicate

> Whether a run/task accepts a mutation (retry, continue, requeue, repair,
> claim) is a function of the run + task FSM state. The UI consumes
> derived booleans, not raw status strings, and any 409 response means the
> projection is stale, not that we need ad-hoc client-side recovery.

**Owns this:** `crates/tandem-server/src/automation_v2/run_mutability.rs`

**Why it matters:** Commit `326c910` added a client-side
`withAutoPauseRetry` helper that catches
`AUTOMATION_V2_RUN_TASK_NOT_MUTABLE` / `AUTOMATION_V2_RUN_NOT_RECOVERABLE`
409s, auto-pauses the run, and retries. That paper-overs a real
disagreement between the run state machine and the UI projection of it.

**Enforcement:** A single pure function `mutability(run, task) ->
RunMutability` derives `{ can_retry, can_continue, can_requeue,
can_repair, can_claim }`. The wire layer publishes the derived booleans;
the UI button is disabled when the boolean is false. The
`withAutoPauseRetry` helper is deleted, not just unused, at the end of
Phase 3.

**Filled by:** Phase 3.

## Phase ordering and exit criteria

| Phase | Scope | Exit criterion |
|---|---|---|
| 0 | This doc + the three module destinations claimed | Skeleton modules merged, three `// TODO(spine)` markers in place |
| 1 | Fill `write_targets.rs` with `ToolKind` + exhaustive match | `extract_session_write_target_paths` and `tool_requires_concrete_write_target` are thin delegates; property test green |
| 2 | Fill `mcp_ready.rs`; route every MCP call through the gate | Raw connection type private to the runtime crate; new caller without the gate is a compile error |
| 3 | Fill `run_mutability.rs`; surface derived booleans on the wire | `withAutoPauseRetry` deleted; UI button gating consumes the booleans |
| 4 (optional) | Regroup `logic_parts/partN.rs` by responsibility | Bug-rate metric drops; otherwise skip |

## Bug-rate metric

After each phase, count commits in the prior two weeks that match
`/^Fix (automation|MCP|preflight|workflow|artifact|write target)/i`. If
the count does not trend down within ~3 weeks of Phase 2 landing, stop:
the invariants picked here are not the ones causing the bugs, and a fresh
audit is warranted before Phase 3.
