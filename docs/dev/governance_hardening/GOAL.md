# Governance Enforcement Hardening — Goal

Last updated: 2026-06-02
Owner: Engine / Runtime Authority
Source audit: `docs/dev/governance_hardening/GOVERNANCE_ENFORCEMENT_HARDENING_PLAN.md`
Kanban: `docs/dev/governance_hardening/KANBAN.md`

## Goal

Make governance checks **actually enforced** on every consequential automation/run
mutation path in the engine runtime, so that the product's core promise — "authority
below the model, evidence that survives" — is true in code and not just in the V2 CRUD
handlers.

A mutation is "consequential" if it can create, update, delete, run, retry, requeue,
resume, approve, reject, rework, cancel, share, or grant capabilities/quota on an
automation or run.

## Definition of done (per path)

For every consequential path:

1. It resolves an **actor/principal** from a non-forgeable source (human vs agent).
2. It resolves **tenant context** and checks target ownership (no IDOR).
3. It calls the **governance/authority layer** before the mutation is persisted/executed.
4. **Human-only** actions are enforced server-side (not labels), including a
   **self-approval guard** (the decider may not be the requesting/executing agent).
5. **Agent-authored** actions are blocked/limited/grant-gated as designed.
6. Every **allowed AND denied** consequential action writes a tamper-evident
   protected audit event attributing **who** (or which system actor) decided.
7. No alternate endpoint can perform the same mutation while skipping 1–6.

## Non-goals (explicit, to bound scope)

- Desktop app and TUI hardening (they are clients; the runtime is the product).
- The Tandem control panel except where it calls hosted enterprise governance APIs.
- New governance *features* (e.g. a brand-new runtime capability-grant request/UX flow)
  beyond wiring existing checks — tracked separately if needed.
- UI polish, broad refactors, or renaming.
- Re-architecting single-tenant → multi-tenant for the local engine (the hosted/
  enterprise plane already carries verified tenant context).

## Guiding constraints

- Prefer routing side-door endpoints through the **existing** governed code path rather
  than duplicating checks.
- Fail closed: a missing/errored governance result must block, not proceed.
- Keep the hosted/enterprise control plane (already protected) as the reference model.
- Each item must ship with tests proving both the allow and the deny path.
