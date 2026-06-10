# Context Evals

Long-session context regression evals (TAN-192) with provenance assertions
(TAN-193). These verify not only that the agent can answer, but **which
context was injected** to produce the answer — context hygiene is governance,
so passing by accidentally injecting too much context is a failure.

## Where the suites live

| Suite | Location | Run with |
| --- | --- | --- |
| Engine-loop evals | `crates/tandem-core/src/engine_loop/tests/context_evals.rs` | `cargo test -p tandem-core context_eval` |
| Hook/memory-scope evals | `crates/tandem-server/src/app/state/tests/mod.rs` (`context_eval_*`) | `cargo test -p tandem-server --lib context_eval` |

Both run locally and in CI with scripted providers — no real provider API
calls, no network.

## How an engine-loop eval works

`run_context_eval(base, seed, prompt)` seeds a session with stored messages,
runs one prompt through the real engine loop against a
`MessagesCaptureProvider`, and returns a `ContextEvalRun` with:

- `final_messages` — the exact provider-facing message vector captured at the
  provider boundary (after history projection, system prompt, hook additions).
- `budget_event` — the `context.budget.final` telemetry for the run.
- `events` — every engine event emitted during the run.

Assert on injected context via `final_context_contains`, on lossy-projection
handles via `assert_compaction_has_provenance_handles`, and on telemetry via
`budget_u64` / `budget_bool` (these panic with the full event payload, so
failures carry actionable context-budget diagnostics).

## Covered scenarios

- **10-turn chat, turn-11 follow-up** — task goal from turn 1 is still in the
  provider-facing context; telemetry confirms no compaction was needed.
- **Approval boundary vs. compaction** — a human approval granted early in a
  60-turn session survives compaction as a pinned projection; raw stored
  history is untouched. Fails if the approval boundary is missing from final
  context.
- **Tool-heavy session** — large intermediate tool outputs are compacted in
  the projection (`toolResultsCompacted` / `toolResultCharsSaved`), the final
  prompt is measurably smaller than raw output volume, and shell exit-status
  tails survive.
- **Compacted history provenance** — the lossy compaction note carries source
  message ranges and stored message ids. Fails if the projection lacks
  handles.
- **Memory scope isolation** (server suite) — with project-scoped memory
  available, unrelated cross-project global memory is not injected, injected
  lines carry record-id provenance handles, and the global hit is deferred
  rather than silently dropped.

Budget fail-closed diagnostics (final prompt over hard budget) are covered by
`full_context_hard_budget_fails_closed_before_provider_send` in
`crates/tandem-core/src/engine_loop/tests/suite_c.rs`.

## Adding a scenario

1. Build seed history with `seed_text_turn` (plain turns) or `Message::new`
   with `MessagePart::ToolInvocation` (tool results).
2. Call `run_context_eval` and assert on `final_messages`, `budget_event`,
   and `events`.
3. Assert provenance, not just presence: if your scenario produces a lossy
   projection (compaction, summarization, tool-result trimming), require
   source handles in it.
4. Keep fixtures free of sensitive raw prompt bodies — captures stay in test
   memory; do not persist them to fixture files.

Related references: `docs/ENGINE_CONTEXT_ASSEMBLY_MAP.md` (what contributes
to final context and the telemetry it emits) and the
`context.budget.final` event fields in
`crates/tandem-core/src/engine_loop/prompt_execution.rs`.
