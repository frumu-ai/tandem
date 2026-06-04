# Implementation Spec — In-Process Eval Engine Bootstrap (stub/live)

Status: `spec / ready to implement` (advice: do this; it's the prerequisite that turns the
Phase 0/1 cross-tenant evals from simulation shape-checks into real enforcement-checks).
Owner: whoever picks up the kanban follow-up. Companion: `PLAN.md`, `KANBAN.md` (CT-01 note).

A dedicated agent can execute this end-to-end from the anchors below. It is a **mechanical
lift of existing working recipes**, not new engine code.

---

## 1. Why & what

`--engine-mode simulation` (the per-PR gate) *echoes* each case's `expected_output`, so a
green run proves shape/regression only, not enforcement. `--engine-mode stub` runs the real
runtime with a deterministic scripted provider — that's the real enforcement signal. Today
the CLI can't do it: `bin/eval_runner.rs::main()` builds `EvalRunner::new(config)` with **no
`AppState`**, so stub/live fall through to `engine_mode_unavailable`
(`crates/tandem-server/src/eval/runner.rs:159-188`, `:302-328`).

Goal: bootstrap a fully-`Ready` in-process `AppState` in the CLI for stub mode, inject the
scripted provider, spawn the automation executor, and attach it to the runner — so
`create_automation_v2_run` → poll-to-terminal (the already-implemented
`EngineExecutor`, `eval/engine_executor.rs`) actually runs.

## 2. Existing recipes to lift (the whole reason this is low-risk)

- **Full `Ready` AppState assembly:** `crates/tandem-server/src/http/tests/mod.rs::test_state()`
  (≈ lines 55-188). Builds every subsystem (storage, config, event_bus, `ProviderRegistry`,
  plugins, agents, tools, permissions, mcp, pty, lsp, workspace_index, cancellations,
  `EngineLoop`), calls `AppState::new_starting(uuid, false)`, sets ~30 `state.*_path` fields
  to a temp root, then `state.mark_ready(RuntimeState { .. }).await` — **this is the step that
  populates the `runtime` OnceLock the executor waits on.**
- **Scripted-provider automation runner (in-process, runs to terminal):**
  `crates/tandem-server/src/app/state/tests/automations/integration_parts/helpers.rs` —
  `install_provider_and_tools()` does the injection:
  `state.providers.replace_for_test(vec![Arc::new(provider.clone())], Some("scripted".to_string())).await;`
- **Injection API:** `ProviderRegistry::replace_for_test(providers, default_provider_id)`
  (`crates/tandem-providers/src/lib_parts/part01.rs:750`, async).
- **Executor loop:** `run_automation_v2_executor(state)`
  (`crates/tandem-server/src/app/state/automation/tasks.rs:15`) — waits for Ready, then
  claims queued runs via `claim_next_queued_automation_v2_run` and executes them.
- **Scripted provider (production-facing):** `crates/tandem-server/src/eval/scripted_provider.rs`
  — `ScriptedEvalProvider::new()`, `.with_pattern(contains, ScriptedResponse)`,
  `.with_default(ScriptedResponse)`; advertises provider id `SCRIPTED_PROVIDER_ID`.
- `RuntimeState`: `crates/tandem-server/src/runtime/state.rs:19`;
  `mark_ready`: `app_state_impl_parts/part01.rs:455`.

## 3. Implementation steps

1. **Extract a reusable, non-test bootstrap.** Pull the AppState assembly out of `test_state()`
   into a non-`cfg(test)` function, e.g. `crate::eval::bootstrap::build_ready_app_state(root:
   PathBuf, opts: BootstrapOpts) -> AppState`, ending in `mark_ready`. Then **have
   `test_state()` delegate to it** (passing test-only seeding: the github MCP tool-cache +
   `mcp.connect("github")`). Keeping `test_state`'s observable behavior identical is the
   acceptance bar for not breaking the suite.
   - Lower-risk alternative if the refactor proves invasive: duplicate the ~90 lines into the
     eval bootstrap with a `// keep in sync with test_state()` note, and skip touching tests.
     Prefer the extraction; fall back to duplication only if the suite goes red.
2. **Inject the scripted provider (stub mode):** after building state,
   `state.providers.replace_for_test(vec![Arc::new(ScriptedEvalProvider::new().with_default(default_resp))], Some("scripted".to_string())).await;`
   The default provider id must be `"scripted"` so node execution routes to it.
3. **Spawn the executor:** `tokio::spawn(run_automation_v2_executor(state.clone()));` before
   running the dataset, so queued runs progress to terminal.
4. **Wire the CLI** (`bin/eval_runner.rs::main`): for `Stub`/`Live` when `engine_url`/`engine_token`
   are NOT provided (local mode), build the bootstrap, spawn the executor, and
   `EvalRunner::new(config).with_app_state(state)`. (Remote mode — url+token set — already works
   via `RemoteEngineExecutor`; leave it.)
5. **Runtime flavor:** `main` is `#[tokio::main(flavor = "current_thread")]`. The executor loop
   and the per-case poll loop must interleave; verify a stub run actually progresses. If it
   stalls, switch to the multi-thread flavor (or `tokio::runtime::Builder`) for stub/live.

## 4. The crux for tenant_isolation (read before claiming a real enforcement test)

For `critical_path` (happy path) the scripted provider just needs responses that satisfy the
node output contracts → runs `Completed`. **For `tenant_isolation`, a green stub run must come
from REAL enforcement, not a scripted "blocked" string.** The scripted provider only returns
model text; the *blocking* has to be produced by a tenant-scoped runtime path the automation
actually exercises (e.g., a node that attempts a tenant-b memory read / source / secret ref
which the runtime denies, yielding `Blocked`). So each `ct_isolation_*` scenario needs its
`automation_spec` to drive a node into a genuinely tenant-scoped call. Until that path is wired,
`tenant_isolation` in stub mode tests "the run reaches terminal," not "cross-tenant access is
denied." Document which it is per scenario. (This is the real depth of CT-02/03/04.)

## 5. Verification

- `cargo run --release --bin eval-runner -- --dataset eval_datasets/critical_path.yaml --engine-mode stub --verbose`
  → runs reach `Completed` (no `engine_mode_unavailable`), validators pass via scripted responses.
- Then `--dataset eval_datasets/tenant_isolation.yaml --engine-mode stub` → reaches `Blocked`
  via a real tenant-scoped denial (see §4).
- Full `tandem-server` test suite stays green (the `test_state` extraction is the only suite risk).
- Wire `tenant_isolation.yaml` into the **stub-baseline** workflow rotation once stub runs are real.

## 6. Risks

- **`test_state` extraction** touches a helper used by ~1600 tests → run the whole suite.
- **Single-thread runtime** (§3.5).
- **Scripted patterns per scenario** + the §4 enforcement-path crux.
- **Governance/config:** confirm `create_automation_v2_run` with `EVAL_TRIGGER_TYPE` is permitted
  (it's the eval path, intended to be allowed).

## 7. Fallback (only if in-process proves intractable)

Remote server + scripted provider: add a `--scripted-provider` (or env) switch to `tandem-server`
startup that injects `ScriptedEvalProvider`, boot it in the stub-baseline workflow, and point
eval-runner at it via `--engine-url`/`--engine-token` (reuses the working `RemoteEngineExecutor`).
More CI moving parts; the server-side switch doesn't exist yet. Not recommended as primary.
