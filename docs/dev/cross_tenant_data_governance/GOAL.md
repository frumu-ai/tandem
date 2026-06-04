# GOAL — Cross-Tenant Data Governance ("Central Brain")

## North star

Make Tandem the trustworthy runtime that can both **block** and **govern the sharing of**
data **across tenants**, with **continuous, eval-backed assurance** — the substrate an
inter-company, AI-native "circular" operating model needs (AI at the center holding a
governed world model; humans at the edge).

Today Tandem is excellent at **isolation** (blocking) and good at **within-tenant governed
access**. It is *not yet* first-class or tested at **governed cross-tenant sharing** (tenant
A deliberately, auditably shares a data class with tenant B under policy, with revocation).
Closing that — on top of a proven-isolated base — is the differentiator.

See `RESEARCH.md` for the full blueprint↔codebase analysis this plan is built on.

## What "done" looks like

1. **Isolation is continuously proven, not just constructed.** Every consequential
   cross-tenant *denial* (data, memory, secrets, audit, agent execution, channels) is
   exercised by an eval that runs in CI on every PR — not only by unit tests.
2. **No silent data-class leakage.** `DataBoundary`/`DataClass` enforcement is on by default
   for governed reads, not only when a `StrictTenantContext` happens to be present.
3. **Cross-tenant sharing is a first-class, signed, revocable grant** — issuer tenant →
   audience tenant, scoped to data classes + permissions + expiry — with both tenants
   attributed in a tamper-evident audit record.
4. **Sharing is eval-backed in the positive direction too:** "A shares X with B → B reads X,
   cannot read Y, every access audited, revocation propagates," proven by evals.
5. **The audit trail is the world-model ledger:** tenant-correct, complete (including
   internal sweeps), queryable per-tenant only.

## Non-negotiable invariants (carry over from the governance hardening work)

- **Local/single-tenant must never break.** Every change is a no-op for the default
  `LocalSingleTenant` / single-tenant operator unless they opt in. (Same constraint that
  shaped GOV-B6a/B10/B5.)
- **Fail closed.** Default-deny on ambiguity; sharing requires an explicit, signed grant.
- **Attribution always.** No consequential cross-tenant action without an attributed,
  tamper-evident audit event.
- **Build on the merged hardening.** PR #1458 closed the human/agent gates, non-forgeable
  actor classification, deny-path audit, channel→tenant binding, etc. This plan assumes and
  extends that substrate; it does not redo it.

## Out of scope (for now)

- Federation/decentralization protocols (cross-deployment, not just cross-tenant).
- Billing/marketplace mechanics of the "circular" business model — this plan delivers the
  *runtime trust substrate* the model needs, not the commercial layer.

## Phases (detail in `PLAN.md`, tracked in `KANBAN.md`)

- **Phase 0 — Prove the isolation we already have.** Wire the drafted cross-tenant denial
  eval into CI + add the highest-value negative evals. Low risk, immediate brand protection.
- **Phase 1 — Close the negative-eval gap list.** The remaining "must block" scenarios +
  an adversarial suite.
- **Phase 2 — Cross-tenant governed sharing (the product move).** First-class signed
  cross-tenant grants, default data-class enforcement, and positive sharing evals.
