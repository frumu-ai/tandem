# PLAN — Cross-Tenant Data Governance

Detailed implementation/verification plan for the goal in `GOAL.md`. Evidence and codebase
anchors are in `RESEARCH.md`. Actionable items are tracked in `KANBAN.md` (IDs `CT-*`).

---

## 1. Current state (what we build on)

- **Tenant identity:** `TenantContext` / `VerifiedTenantContext` (Ed25519 JWS), deployment
  modes `LocalSingleTenant` / `HostedSingleTenant` / `EnterpriseRequired`
  (`tandem-enterprise-contract/src/lib.rs`, `tandem-server/src/http/middleware.rs`).
- **Isolation:** `tenant_matches()` on the `(org_id, workspace_id, deployment_id)` triple;
  filter-then-access everywhere; memory partitioned at storage + query.
- **Within-tenant governed access:** `StrictTenantContext` → `ScopedGrant` →
  `DataBoundary`/`DataClass` (`lib.rs:1159-1462`). **Grants are intra-tenant and embedded in
  the verified context; there is no A→B grant and no runtime cross-tenant grant lookup.**
- **Data-class enforcement is conditional** — only when a `StrictTenantContext` is present.
- **Governance substrate hardened by PR #1458** (merged): non-forgeable actor, human-only
  gates/approvals, deny-path protected audit, governed share, ownership checks, launch-time
  spend recheck, channel→tenant binding, consequential-route regression guard.
- **Eval framework:** `crates/tandem-server/src/eval/*`, `eval_datasets/*.yaml`,
  `eval_baselines/main_branch.json`, CI `eval-regression-gate.yml` (simulation, per-PR) and
  `eval-stub-baseline.yml` (stub, weekly). Test case = embedded `automation_spec` +
  `expected_output` (status, validators, quality indicators); score = pass/total +
  validator pass-rates + failure modes.
- **Cross-tenant test coverage today:** strong unit/integration *isolation* tests
  (routines, sessions, providers, context-runs, enterprise, ~20 memory-DB tests); **one
  drafted eval** `eval_datasets/fintech_compliance_risk.yaml:129` (`fintech_004`,
  cross-tenant denial + audit) that is **not yet in the CI baseline**.

---

## 2. Design principles for this work

1. **Evals are the deliverable, not an afterthought.** Each "must block" guarantee gets an
   eval that runs in the regression gate, so isolation is *continuously proven*. Prefer
   eval-dataset coverage (runs in CI gate) over only Rust unit tests where the scenario is
   end-to-end.
2. **Negative before positive.** Prove blocking comprehensively (Phase 0/1) before shipping
   sharing (Phase 2). A sharing feature on an unproven-isolated base is a liability.
3. **Local-safe by construction.** Every enforcement default must be a no-op for
   single-tenant/local (mirror GOV-B6a/B10 reasoning): only engage when a real second
   tenant / verified context / explicit grant is present.
4. **Signed + attributed.** Cross-tenant sharing is expressed as a signed grant and every
   cross-tenant access is audited with *both* tenants attributed.
5. **Fail closed.** Absence of an explicit grant = deny. Default `DataBoundary` for governed
   reads denies unclassified-but-sensitive flow rather than allowing by omission.

---

## 3. Phase 0 — Prove the isolation we already have

Goal: turn "isolation by construction" into "isolation continuously proven in CI," cheaply.

### 3.1 Wire the drafted cross-tenant denial eval into CI (`CT-01`)
- `fintech_004` already encodes "cross-tenant source/secret access fails closed + emits
  audit evidence" but is not in `critical_path`/the baseline, so it never runs in the gate.
- Create a dedicated dataset `eval_datasets/tenant_isolation.yaml` (don't overload
  `critical_path`), move/duplicate `fintech_004` into it, ensure the simulation engine
  produces a deterministic `blocked` outcome with the `tenant_scope` + `audit_event`
  validators, and add it to the regression gate (`eval-regression-gate.yml`) + a baseline in
  `eval_baselines/`.
- **Decision D-CT-1:** does the regression gate run multiple datasets, or one? Likely extend
  the workflow to run a `tenant_isolation.yaml` dataset alongside `critical_path.yaml`.

### 3.2 Top negative evals (the highest-value gaps)
- `CT-02` **Agent/tool execution cross-tenant:** an agent running in tenant A must not reach
  tenant B's memory/tools/secrets *at runtime* (today only route-level is tested). Model as
  an automation whose node attempts a tenant-B resource ref / memory read → expect
  `blocked` + audit.
- `CT-03` **Memory promotion re-scope:** session→project→global promotion must re-scope to
  the owning tenant; a promoted chunk must never become visible to another tenant. Eval +
  targeted `tandem-memory` test.
- `CT-04` **Audit cross-tenant visibility:** tenant B cannot read tenant A's audit events.
  (`fintech_004` *requires* an `audit_event` validator but nothing tests audit isolation
  itself.) Likely a Rust integration test on the audit read path + an eval validator.

Exit criteria for Phase 0: cross-tenant denial + the three top negative scenarios run green
in the CI regression gate on every PR.

---

## 4. Phase 1 — Close the negative-eval gap list

Each item = a "must block" scenario with an eval (and a Rust test where the surface is not
reachable from the eval harness). All are negative ("expect blocked + audit").

- `CT-05` **Channel/webhook cross-tenant routing:** Slack/Discord/Telegram interaction from
  org A must not act on org B's run (builds on the merged B5c channel→tenant binding; add
  the eval/test that proves it).
- `CT-06` **Provider quota/error leakage:** single-tenant fallback + cross-tenant — tenant B
  cannot use tenant A's provider credential; provider error messages don't leak secrets;
  rate-limit/quota state is not shared across tenants.
- `CT-07` **MCP secret cross-tenant injection:** a store-backed MCP secret reference is
  validated against the requesting tenant before lookup; cross-tenant ref → blocked.
- `CT-08` **Knowledge/skill cross-tenant:** tenant B cannot retrieve/execute tenant A's
  knowledge base or skills; promotion re-binds tenant.
- `CT-09` **Governance approval-receipt reuse:** an approval issued in tenant A cannot be
  replayed to authorize an action in tenant B; revocation is tenant-scoped.
- `CT-10` **Adversarial suite:** forged tenant-context assertion is rejected (JWS
  verification); noisy-neighbor resource starvation (one tenant exhausting shared
  token/memory budget) is bounded and attributed.

Exit criteria: the full negative matrix runs in CI (simulation gate for deterministic ones;
stub baseline for engine-dependent ones).

---

## 5. Phase 2 — Cross-tenant governed sharing (the product move)

This is where the "central brain" stops being "secure multi-tenant app" and becomes "the
governed interchange layer." Design-first; needs decisions before implementation.

### 5.1 First-class cross-tenant grant (`CT-11` design → `CT-13` implement)
- Extend the grant model so a grant can be **issued by tenant A to tenant B**:
  - fields: issuer tenant, audience tenant, principal (optional specific actor), resource
    scope, **data classes**, permissions, expiry, revocation handle, signature.
  - representation: extend `ScopedGrant` / `StrictTenantContext`, or a new
    `CrossTenantGrant` type carried in / referenced by the verified context.
  - lookup: a runtime grant store keyed by `(audience_tenant)` resolving inbound grants,
    verified by signature — the first *intentional* cross-tenant lookup in the system.
- **Decision D-CT-2:** grant representation — extend `ScopedGrant` (audience field) vs. a new
  `CrossTenantGrant`. **D-CT-3:** issuance + revocation surface (enterprise admin API? signed
  by the issuer's key?). **D-CT-4:** trust root — same Ed25519 context-assertion keys, or a
  separate grant-signing key per tenant?

### 5.2 Default data-class enforcement (`CT-12`)
- Make `DataBoundary`/`DataClass` checks apply to governed reads **by default**, not only
  when a `StrictTenantContext` is present — so the world model can't leak by omission.
- **Local-safe:** default boundary for single-tenant/no-verified-context = allow (no-op);
  enforcement engages when a verified/multi-tenant context or a cross-tenant grant is in
  play. **Decision D-CT-5:** the default boundary policy + the exact trigger condition.

### 5.3 Positive sharing evals (`CT-14`)
- "A shares data class X with B under policy → B can read X, **cannot** read Y (different
  class / out of scope), every access audited with **both** tenants attributed, and
  **revocation propagates** (a follow-up eval proves B loses access)."
- This is the first eval suite that asserts *successful* cross-tenant flow, not just denial.

### 5.4 Audit trail as world-model ledger (`CT-15`)
- Ensure the protected audit trail is tenant-correct, complete on internal sweeps (extends
  the merged B8a deny-path audit toward B8b sweep attribution), and queryable per-tenant
  only — so it can serve as the "record every decision/plan/approval" substrate the
  blueprint's world model requires.

Exit criteria: a tenant can grant another tenant scoped, data-class-bounded, revocable,
audited access to a resource, and the positive + revocation evals run green in CI.

---

## 6. Open decisions (consolidated)

| ID | Decision | Blocks |
|----|----------|--------|
| D-CT-1 | Regression gate: one dataset or multiple? Add `tenant_isolation.yaml` to the gate | CT-01 |
| D-CT-2 | Cross-tenant grant representation: extend `ScopedGrant` vs. new `CrossTenantGrant` | CT-11/13 |
| D-CT-3 | Grant issuance + revocation surface (API, who signs) | CT-11/13 |
| D-CT-4 | Trust root for grants (reuse context-assertion keys vs. per-tenant grant keys) | CT-11/13 |
| D-CT-5 | Default `DataBoundary` policy + the trigger for default enforcement (local-safe) | CT-12 |

Phase 0 and most of Phase 1 are **not blocked** by any decision and can start immediately.
Phase 2 needs D-CT-2..5 resolved first.

---

## 7. Sequencing & risk

1. **Phase 0 first** — small, low-risk, immediate brand protection (proves no leaks in CI).
2. **Phase 1** — steady negative-eval buildout; each item independent and parallelizable.
3. **Phase 2** — gated on the design decisions; build sharing on the proven-isolated base.

Risk controls: every change verified local-safe (single-tenant no-op); positive sharing
ships only after the negative matrix is green; data-class default enforcement rolled out
behind the local-safe trigger to avoid breaking existing single-tenant deployments.
