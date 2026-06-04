# KANBAN — Cross-Tenant Data Governance

Tracks the work in `PLAN.md` toward the `GOAL.md` north star. Companion: `RESEARCH.md`.

## Status Legend
- `todo`
- `in_progress`
- `research` (investigated; needs a decision before implementation)
- `blocked`
- `done`

## Priority Legend
- `P0` — prove the isolation we already have (brand-critical; cheap)
- `P1` — close the negative ("must block") eval gap list
- `P2` — cross-tenant governed sharing (the product move; design-gated)

## Conventions
- Prefer **eval-dataset** coverage (runs in the CI regression gate) over Rust-only tests for
  end-to-end scenarios; add Rust tests where the surface isn't reachable from the eval harness.
- Every item must be **local/single-tenant no-op** unless a real second tenant / verified
  context / explicit grant is present.
- Every "must block" item asserts **blocked + attributed audit event**.

---

## Now (Phase 0 — P0)

- `CT-01` Wire the drafted cross-tenant denial eval into CI
  - Status: `done`
  - Priority: P0
  - Scope: `fintech_004` ("cross-tenant source/secret access fails closed + emits audit evidence") existed but was not in the CI baseline, so it never ran in the gate.
  - Decision **D-CT-1 resolved:** the gate runs multiple datasets — a dedicated `tenant_isolation.yaml` runs alongside `critical_path.yaml`, each with its own baseline + regression check (rather than overloading `critical_path`).
  - Progress: created `eval_datasets/tenant_isolation.yaml` (two platform-level cross-tenant *denial* scenarios: source/secret access `ct_isolation_001`, memory read `ct_isolation_002`), generated `eval_baselines/tenant_isolation.json`, and wired it into `.github/workflows/eval-regression-gate.yml` (run + pass-rate regression check + PR-comment line + artifact + fail-on-regression).
  - **Honest scope (read this):** the per-PR gate runs in `--engine-mode simulation`, which **echoes** each case's `expected_output` — so a green run proves the dataset is present, well-formed, and shape-stable vs. baseline; it does NOT by itself prove the system blocks cross-tenant access. Real enforcement is proven today by the Rust integration tests (`tenant_a_cannot_*`, `tandem-memory` tenant-scoped tests). True real-engine eval coverage needs `--engine-mode stub/live`, which the eval CLI does **not** bootstrap yet (`runner.rs` doc: "CLI does not yet bootstrap an AppState"). That framework bootstrap is a separate follow-up (see note below); not touched here to avoid changing a non-functional path.
  - Files: `eval_datasets/tenant_isolation.yaml`, `eval_baselines/tenant_isolation.json`, `.github/workflows/eval-regression-gate.yml`.
  - Verification: `./target/release/eval-runner --dataset eval_datasets/tenant_isolation.yaml --simulation` → pass_rate 1.0, validators `[tenant_scope, audit_event]`; gate runs + regression-checks it on every PR.
  - Follow-up (new): `CT-0X` bootstrap stub/live `AppState` in the eval CLI so cross-tenant denial runs against the real engine in CI (turns Phase 0/1 evals from shape-checks into enforcement-checks).

- `CT-02` Eval: agent/tool execution must not cross tenants at runtime
  - Status: `todo`
  - Priority: P0
  - Scope: route-level isolation is tested, but a *running* agent in tenant A reaching tenant B's memory/tools/secrets is not.
  - Acceptance: an eval automation whose node attempts a tenant-B resource/memory/secret ref resolves to `blocked` + audit; agent cannot escape its tenant during execution.
  - Files: new eval scenario in `eval_datasets/tenant_isolation.yaml`; `crates/tandem-server/src/eval/spec_mapper.rs`; runtime authority in `app/state/*`.
  - Verification: eval green in simulation + (where engine-dependent) stub baseline.

- `CT-03` Eval/test: memory promotion re-scopes tenant (no leak)
  - Status: `todo`
  - Priority: P0
  - Scope: session→project→global promotion must re-scope to the owning tenant; a promoted chunk must never become visible to another tenant.
  - Acceptance: promotion preserves tenant scope; cross-tenant read of a promoted chunk returns nothing; covered by eval + a targeted `tandem-memory` test.
  - Files: `crates/tandem-memory/src/*` (promotion path), `crates/tandem-memory/src/db.rs` (tests), eval scenario.
  - Verification: new memory test + eval green.

- `CT-04` Audit cross-tenant visibility (negative)
  - Status: `todo`
  - Priority: P0
  - Scope: `fintech_004` requires an `audit_event` validator but nothing tests that tenant B cannot read tenant A's audit events.
  - Acceptance: audit read/query path is tenant-scoped; tenant B cannot see tenant A's protected audit events.
  - Files: `crates/tandem-server/src/audit.rs` + audit read/query path; `http/tests/` integration test; eval validator.
  - Verification: integration test asserts tenant-scoped audit reads.

---

## Next (Phase 1 — P1, negative eval gap list)

- `CT-05` Channel/webhook cross-tenant routing
  - Status: `todo`
  - Priority: P1
  - Scope: a Slack/Discord/Telegram interaction from org A must not act on org B's run (the merged B5c added channel→tenant binding; add the eval/test proving it).
  - Acceptance: cross-tenant channel interaction → rejected + audited.
  - Files: `crates/tandem-server/src/http/{slack,discord,telegram}_interactions.rs`; `http/tests/`.

- `CT-06` Provider quota/error/credential cross-tenant leakage
  - Status: `todo`
  - Priority: P1
  - Scope: tenant B cannot use tenant A's provider credential; provider error messages don't leak secrets; rate-limit/quota state isn't shared across tenants; single-tenant fallback isolation.
  - Files: provider auth + rate-limit paths; `http/tests/providers.rs`.

- `CT-07` MCP secret cross-tenant injection
  - Status: `todo`
  - Priority: P1
  - Scope: store-backed MCP secret references validate the requesting tenant before lookup; cross-tenant ref → blocked.
  - Files: MCP secret resolution path; eval scenario with a protected MCP tool.

- `CT-08` Knowledge/skill cross-tenant retrieval/promotion
  - Status: `todo`
  - Priority: P1
  - Scope: tenant B cannot retrieve/execute tenant A's knowledge base or skills; promotion re-binds tenant.
  - Files: knowledge/skill stores + `eval/spec_mapper.rs` (KnowledgeBinding).

- `CT-09` Governance approval-receipt reuse across tenants
  - Status: `todo`
  - Priority: P1
  - Scope: an approval issued in tenant A cannot be replayed to authorize an action in tenant B; revocation is tenant-scoped.
  - Files: governance approval path (`http/governance.rs`, `routes_governance.rs`).

- `CT-10` Adversarial suite
  - Status: `todo`
  - Priority: P1
  - Scope: forged tenant-context assertion is rejected (JWS); noisy-neighbor resource starvation (token/memory budget) is bounded and attributed.
  - Files: `http/middleware.rs` (assertion verification), rate-limit/budget paths.

---

## Backlog (Phase 2 — P2, cross-tenant governed sharing)

- `CT-11` Design: first-class cross-tenant grant
  - Status: `research`
  - Priority: P2
  - Scope: a signed grant issued by tenant A to tenant B — issuer/audience tenant, principal, resource scope, data classes, permissions, expiry, revocation, signature; plus an inbound-grant lookup keyed by audience tenant.
  - Acceptance: a written design (representation, issuance/revocation surface, trust root, lookup + enforcement points) approved before implementation.
  - Decisions: D-CT-2 (extend `ScopedGrant` vs. new `CrossTenantGrant`), D-CT-3 (issuance/revocation surface), D-CT-4 (grant trust root).
  - Files: `tandem-enterprise-contract/src/lib.rs` (grant types), `http/middleware.rs` (context assembly).

- `CT-12` Default data-class (`DataBoundary`) enforcement
  - Status: `research`
  - Priority: P2
  - Scope: apply `DataBoundary`/`DataClass` checks to governed reads by default, not only when a `StrictTenantContext` is present — local-safe (no-op for single-tenant/no-verified-context).
  - Acceptance: governed reads enforce data classes by default for verified/multi-tenant/granted contexts; single-tenant unaffected.
  - Decision: D-CT-5 (default boundary policy + trigger condition).
  - Files: `tandem-enterprise-contract/src/lib.rs:1159-1462` (evaluate_access), `tandem-memory` access filter.

- `CT-13` Implement cross-tenant grant issuance + enforcement
  - Status: `blocked` (on CT-11 design)
  - Priority: P2
  - Scope: implement the approved design — issuance, signing, audience-tenant lookup, enforcement at read paths, revocation.
  - Acceptance: tenant A can grant tenant B scoped, data-class-bounded, revocable access; enforced at memory/resource reads; both tenants attributed in audit.

- `CT-14` Positive cross-tenant sharing evals
  - Status: `blocked` (on CT-13)
  - Priority: P2
  - Scope: "A shares class X with B → B reads X, cannot read Y, every access audited with both tenants, revocation propagates."
  - Acceptance: positive + revocation evals run green in CI (first evals asserting *successful* cross-tenant flow).

- `CT-15` Audit trail as world-model ledger
  - Status: `todo`
  - Priority: P2
  - Scope: tenant-correct, complete-on-internal-sweeps (extends merged B8a deny-path audit toward sweep attribution / B8b), queryable per-tenant only.
  - Files: `crates/tandem-server/src/audit.rs`; internal sweep sites (`app_state_impl_parts/part05.rs` reaper/auto-resume/recover).

---

## Done

_(none yet — branch just created off `main` after PR #1458 merged)_
