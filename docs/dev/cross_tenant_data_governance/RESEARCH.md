# Tandem as the "Central Brain" for the Circular AI-Native Model

Research report. Combines (a) the Dorsey / Sequoia / Redpoint blueprint for AI-native
companies with (b) a codebase audit of Tandem's cross-tenant data-sharing/blocking and
eval coverage, and (c) what to build/test to make Tandem the runtime that blueprint needs.

Date: 2026-06-04. Status: research / not implementation.

---

## 0. TL;DR

- **The blueprint** (Dorsey + Botha/Sequoia + Redpoint, Mar–Apr 2026): replace the
  pyramid org chart with a **circle** — AI at the center as the primary decision-maker
  ("AI+"), humans at the edge, continuous (not quarterly) planning, ~3 roles, hire for
  taste. The engine is a continuously-updated **"world model"** of the company plus a
  strong **customer signal**, which requires **recording and tracking every decision,
  discussion, plan, problem, and approval**. AI performs the coordination work middle
  management used to do.
- **The implication the article doesn't supply:** that circle needs a **runtime** that is
  simultaneously the *authority* (who/what may act), the *memory* (the world model), the
  *coordinator* (agents + approvals), and the *record* (tamper-evident audit). That is
  precisely Tandem's stated identity — "the authority layer for AI-first work: runtime
  authority for agents, tools, memory, approvals, and audit trails."
- **The multi-tenant extension (the "central brain across companies"):** a single Tandem
  instance serving many AI-native companies as **tenants**, where data is **governed —
  shared or blocked — across tenant boundaries** under policy + crypto attribution. That
  cross-tenant governance is the flywheel substrate and Tandem's differentiator.
- **Codebase reality:** Tandem is **excellent at blocking** (isolation is strong and
  broadly tested) and good at **within-tenant governed access** (signed grants, data
  classes). It is **not yet a first-class, tested flow at *governed cross-tenant
  sharing*** (tenant A deliberately, auditably shares data-class X with tenant B). That is
  the gap between the blueprint and the product.
- **Eval reality:** 5 happy-path evals in CI; ~30+ unit/integration *isolation* tests
  proving "A cannot touch B"; **one drafted cross-tenant eval (`fintech_004`) not yet in
  the CI baseline**; and a long list of untested cross-tenant scenarios — including **zero
  evals for the positive "governed sharing" direction** (because the feature is nascent).
- **Net:** the trust substrate is real and — after PR #1458's governance hardening —
  substantially stronger than the older audit docs imply. To realize the central-brain
  vision, the work is (1) make cross-tenant *sharing* a first-class signed grant, (2) turn
  data-class enforcement on by default, and (3) convert "isolation by construction" into
  "continuously eval-backed assurance," including positive sharing evals.

---

## 1. The blueprint: the "circular" AI-native company

From the Dorsey/Botha essay (Sequoia) and Redpoint's 2026 outlook:

- **Shape:** pyramid org chart → **circle**. AI at the center; people at the edge.
- **Decision-maker:** "AI+" — AI as the primary decision-maker; humans set taste/direction
  and handle exceptions.
- **Cadence:** continuous planning, not quarterly cycles.
- **Roles:** radical simplification (Dorsey floats ~three roles); **hire for taste**, not
  résumé.
- **Engine:** a company needs a **"world model"** of its own operations and a strong
  **"customer signal."** Building the world model requires a way to **record and track all
  decisions, discussions, plans, problems, and progress** — an ever-evolving ledger.
- **What changed:** AI can now do the **coordination** work that middle management existed
  to do (routing, status, approvals, handoffs).
- **Market timing (Redpoint):** 2026 is "the year pilots convert or quietly disappear";
  AI-native apps leapfrog both legacy incumbents and first-gen AI startups.

**The unstated dependency:** every one of those bullets is a *runtime* requirement. "AI at
the center deciding" needs an **authority layer** (who/what may act, and gates for the
consequential calls). "Record every decision/plan/approval" needs **memory + a
tamper-evident audit trail**. "Coordination work" needs an **agent/automation orchestrator
with approvals**. "Customer signal + world model that compounds" needs **governed data
that can flow** — and, at the network scale, flow *between* organizations under policy. A
spreadsheet, a CRM, or a chat app cannot be that center; a governed runtime can.

---

## 2. Why the circular model needs a runtime like Tandem (blueprint → primitive map)

| Blueprint requirement | Tandem primitive (today) | File anchors |
|---|---|---|
| AI at center as decision-maker; humans handle exceptions | Governance engine: human/agent actor classification, gates, approvals, capability/spend governance | `crates/tandem-server/src/http/governance.rs`, `app/state/governance.rs`, `automation_v2/governance.rs` |
| Record every decision / plan / approval / progress (the "world model") | Tiered, tenant-scoped **memory** + **protected audit trail** | `crates/tandem-memory/*`, `crates/tandem-server/src/audit.rs` |
| Coordination work middle management did | Automations v2, agent teams, runtime authority, gates/approvals | `routines_automations*`, `agent_teams*`, `app/state/automation/*` |
| Who may act on what data (trust) | `TenantContext`/`VerifiedTenantContext` (Ed25519 JWS), `StrictTenantContext`, `DataBoundary`/`DataClass`, `ScopedGrant` | `tandem-enterprise-contract/src/lib.rs`, `http/middleware.rs` |
| Compounding across companies (the flywheel) | Multi-tenant isolation **+ (nascent) cross-tenant governed sharing** | see §3 |

The first four rows are real and shipping. The fifth row — *governed data movement across
tenants* — is where the blueprint's "flywheel" lives and where Tandem is still mostly
**isolation-only**.

---

## 3. Technical state — cross-tenant data sharing & blocking (the trust substrate)

### 3.1 Tenant identity & verification
- `TenantContext` = `org_id` + `workspace_id` (+ optional `deployment_id`, `actor_id`,
  `source`). `VerifiedTenantContext` adds a signed `human_actor`, `authority_chain`,
  `roles`, `capabilities`, and an optional `strict_projection`.
  (`tandem-enterprise-contract/src/lib.rs:903-1156`)
- Verification is **Ed25519 JWS** (`typ: tandem-tenant-context+jws`), keys loaded from
  env, with audience/resource-scope/expiry/issuer constraints.
  (`crates/tandem-server/src/http/middleware.rs:542-734`)
- Deployment modes: **LocalSingleTenant** (unsigned headers, dev), **HostedSingleTenant**
  / **EnterpriseRequired** (JWS mandatory, raw headers forbidden).

### 3.2 Isolation (blocking) — strong and pervasive
- Universal pattern: every enterprise resource implements `tenant_matches()` on the
  `(org_id, workspace_id, deployment_id)` triple; handlers **filter-then-access** and never
  query outside the current tenant. (`routes_enterprise.rs:260-380`,
  `tandem-enterprise-contract/src/lib.rs:1640-1809`)
- Tenant-scoped resource types: sessions, automations/v2, context runs, providers/secrets,
  channels, enterprise org-units/source-bindings, memory.
- **Memory** is partitioned at **storage** (`MemoryTenantScope` = org/ws/deploy;
  `MemoryPartition` key `{org}/{ws}/{project}/{tier}`) and at **query** (`MemoryAccessFilter`
  applies `StrictTenantContext.evaluate_access` post-retrieval).
  (`tandem-memory/src/types.rs:46-161`, `governance.rs:28-80`)
- **Secrets** validate exact `(org_id, workspace_id)` before use
  (`lib.rs:1480-1488`).

### 3.3 Governed access (sharing) — within tenant today
- The sophisticated primitive is `StrictTenantContext` → `ScopedGrant` →
  `DataBoundary`/`DataClass` (10 classes: Public…Credential…Regulated…SourceCode…). Access
  is evaluated as: not expired → data-class allowed by boundary → resource in scope → no
  deny grant → matching allow grant with the right permission + data class.
  (`lib.rs:1159-1462`)
- **Grant sources** include `OrganizationUnitMembership`, `Delegation`, `ExecutiveGlobal`,
  `BreakGlass` — but all grants are **embedded in the verified context and scoped to the
  same tenant**. There is **no first-class "tenant A grants tenant B access to data-class
  X" flow**, and no runtime cross-tenant grant lookup.
- **Enforcement is conditional:** `DataBoundary`/data-class checks only bite when a
  `StrictTenantContext` is present; the OSS/default path does not enforce them.

### 3.4 The governance substrate is now hardened (PR #1458 correction)
The older audit/hardening docs (`docs/dev/governance_hardening/`) enumerate gaps B1–B10
(forgeable actor classification, side-door endpoints bypassing governance, self-approval,
no deny-path audit, ungoverned share, etc.). **Most of these are now CLOSED** by PR #1458:
non-forgeable actor classification (B3), human-only gate/approval + no self-review (B1/B4),
governed+audited share (B7), OSS ownership check (B10a), launch-time spend recheck (B6a),
run_now/cancel attribution + altitude (B9/B2d), deny-path protected audit (B8a), channel
approve-by-default + per-identity step-up + **channel→tenant binding** (B5a/b/c), and a
consequential-route regression guard (X1). **So the brain's "who-can-do-what + tamper-
evident record" substrate is materially stronger than the audit docs suggest** — exactly
the foundation the central-brain model assumes.

---

## 4. Eval state — how sharing/blocking is tested, and the gaps

### 4.1 The eval framework
- Code: `crates/tandem-server/src/eval/{mod,runner,dataset,metrics}.rs`, CLI
  `src/bin/eval_runner.rs`. Datasets: `eval_datasets/*.yaml`. Baseline:
  `eval_baselines/main_branch.json`.
- Modes: **simulation** (deterministic, the PR regression gate), **stub** (real engine +
  scripted provider), **live** (real provider). CI:
  `.github/workflows/eval-regression-gate.yml` (per-PR, simulation),
  `eval-stub-baseline.yml` (weekly, stub).
- Authoring: a test case embeds an `automation_spec` + `expected_output` (artifact status,
  required validators, repair iterations, quality indicators) + tags. Scoring =
  `passed` boolean + validator pass-rates + `AIFailureMode` tracking; overall = pass/total.
- **The CI "5/5" = `eval_datasets/critical_path.yaml`** — five happy-path scenarios
  (research, code-gen, summarization, branching workflow, error recovery). None are
  cross-tenant.

### 4.2 Cross-tenant coverage that EXISTS
- **Isolation is well tested at the unit/integration layer** — "A cannot touch B":
  - Automations v2: `tenant_a_cannot_access_tenant_b_automation_v2_routes`
    (`http/tests/routines.rs:1126`), `automation_v2_payload_cannot_override_request_tenant`
    (`:1417`).
  - Sessions: `tenant_a_cannot_access_tenant_b_session_routes`
    (`sessions_parts/part01.rs:367`).
  - Providers/secrets: `tenant_a_cannot_list_update_or_delete_tenant_b_provider_api_key`
    (`providers.rs:205`), `…_oauth_credential` (`:452`).
  - Context runs: `…front_door_routes` / `…internal_routes`
    (`context_runs_parts/part01.rs:366,486`).
  - Enterprise: `enterprise_org_units_do_not_cross_tenant_boundaries` (`enterprise.rs:164`),
    `…source_bindings_reject_cross_tenant_resource_ref` (`:2009`), `…do_not_cross…` (`:2035`).
  - Memory: `tenant_a_cannot_search_list_delete_demote_or_promote_tenant_b_memory`
    (`memory_parts/part03.rs:1365`) **plus ~20 storage-layer tests** in
    `tandem-memory/src/db.rs:1022-2100` (vector top-k partitioned before ranking, no
    cross-tenant dedupe, tenant-scoped delete/stats/cleanup/global memory).
- **One drafted cross-tenant EVAL, not yet wired into CI:** `fintech_004` in
  `eval_datasets/fintech_compliance_risk.yaml:129-165` — *"Cross-tenant source or secret
  access should fail closed and emit audit evidence"* (`tenant_id: tenant-a`,
  `forbidden_tenant_id: tenant-b`, expected status `blocked`, validators
  `[tenant_scope, audit_event]`, quality `[cross_tenant_denied, audit_event_recorded]`).
  **This is almost certainly the "started but still needs testing" work** — it exists but
  is not in `critical_path`/the baseline, so it doesn't run in the gate.

### 4.3 The gaps (what "still needs to be tested")
Negative ("must block") scenarios with **no eval/test today**:
1. **Agent/tool execution crossing tenants at runtime** (route-level is covered; *running*
   agent reaching tenant B's memory/tools/secrets is not).
2. **Memory promotion leak** — session→project→global promotion without re-scoping tenant.
3. **Audit cross-tenant visibility** — `fintech_004` *requires* an `audit_event` validator,
   but nothing tests that tenant B cannot read tenant A's audit events.
4. **Channel/webhook cross-tenant routing** — Slack/Discord/Telegram interaction from org A
   triggering work in org B (note: PR #1458 added channel→tenant *binding* B5c, but there's
   no eval).
5. **Provider quota/error leakage** and **MCP secret cross-tenant injection**.
6. **Knowledge/skill cross-tenant retrieval/promotion.**
7. **Governance approval-receipt reuse across tenants.**
8. **Adversarial:** forged tenant-context assertion; resource-starvation (one tenant
   exhausting shared token/memory budget to degrade another).
9. **The big one for the business model:** there are **no evals for the *positive* governed
   cross-tenant *sharing* direction** — because that feature is nascent (§3.3).

---

## 5. The blueprint↔product gap, and what it implies

The circular/central-brain model's flywheel requires **data to circulate (under
governance) across the brain's tenants**: the "world model" gets richer as more
organizations contribute signal, and value compounds. Today Tandem can **block** (isolate)
and can do **within-tenant governed access**, but the **cross-tenant governed-sharing
primitive** — *A deliberately, auditably shares data-class X with B under policy, with
revocation* — is **not a first-class, enforced, or tested flow**. That is the single
biggest gap between "Tandem the secure multi-tenant app" and "Tandem the central brain for
an inter-company AI-native network."

This is also the *opportunity*: isolation is table stakes; **governed sharing with
cryptographic attribution and eval-backed assurance** is the moat. The Dorsey/Sequoia/
Redpoint blueprint assumes a trustworthy world-model substrate but doesn't provide one —
that is exactly the runtime layer Tandem is positioned to be.

---

## 6. Recommendation — a staged plan

**Phase 0 — turn on the assurance you already have (days):**
- Promote `fintech_004` into `critical_path` (or a `tenant_isolation.yaml` dataset) and the
  CI baseline so cross-tenant denial + audit evidence runs on every PR.
- Add the top negative evals: agent-execution-cross-tenant, memory-promotion re-scope,
  audit cross-tenant visibility. (Converts "isolation by construction" → "isolation
  continuously proven.")

**Phase 1 — close the negative-eval gap list (weeks):**
- Channel/webhook cross-tenant, provider quota/error leakage, MCP secret injection,
  knowledge/skill cross-tenant, approval-receipt reuse, and an **adversarial** suite
  (forged assertion, resource starvation). Wire into the stub/live baselines.

**Phase 2 — make cross-tenant *governed sharing* a first-class primitive (the product
move):**
- Extend `ScopedGrant`/`StrictTenantContext` to express **A→B grants** (issuer tenant,
  audience tenant, data-class, permissions, expiry, revocation), signed and auditable.
- **Turn `DataBoundary`/data-class enforcement on by default** (not only when a
  `StrictTenantContext` is present), so the world model cannot leak by omission.
- Add **positive** sharing evals: "A shares class X with B under policy → B can read X,
  cannot read Y, every access is audited with both tenants attributed, revocation
  propagates and is proven by a follow-up eval."
- Treat the **protected audit trail as the world-model ledger** (the blueprint's "record
  every decision/plan/approval") — ensure it is tenant-correct, complete on deny paths
  (B8a done; extend to internal sweeps per B8b), and queryable per-tenant only.

**Why this ordering:** Phase 0/1 protect the brand-critical promise (no leaks) cheaply and
immediately; Phase 2 builds the actual differentiator (governed circulation) on top of a
proven-isolated base — which is the only safe order for a system whose value proposition is
trust.

---

## Sources

- [Dorsey, Sequoia and Redpoint Lay Out a New Playbook for AI-Native Companies (Forbes / Yahoo mirror)](https://malaysia.news.yahoo.com/dorsey-sequoia-redpoint-lay-playbook-194720369.html)
- [Jack Dorsey: Every Company Can Now Be a Mini-AGI (Sequoia Capital)](https://sequoiacap.com/podcast/jack-dorsey-every-company-can-now-be-a-mini-agi/)
- [Jack Dorsey and Roelof Botha think AI can make middle management obsolete (Fortune)](https://fortune.com/2026/04/02/jack-dorsey-roelof-botha-ai-middle-management/)

## Codebase anchors (primary)
- Tenant model / data boundary: `crates/tandem-enterprise-contract/src/lib.rs:903-1462`
- JWS verification: `crates/tandem-server/src/http/middleware.rs:542-734`
- Memory partition/filter: `crates/tandem-memory/src/types.rs:46-161`, `db.rs:1022-2100`
- Eval framework: `crates/tandem-server/src/eval/*`, `eval_datasets/*.yaml`, `eval_baselines/main_branch.json`
- Drafted cross-tenant eval: `eval_datasets/fintech_compliance_risk.yaml:129-165`
- Governance hardening (mostly closed by PR #1458): `docs/dev/governance_hardening/`
