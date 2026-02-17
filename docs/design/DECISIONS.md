# Decisions

## Format

| Date | Decision ID | Work IDs | Decision | Rationale | Impact |
| ---- | ----------- | -------- | -------- | --------- | ------ |

## Decision Log

| Date       | Decision ID | Work IDs            | Decision                                                                                                                                 | Rationale                                                                                   | Impact                                                                                                                                        |
| ---------- | ----------- | ------------------- | ---------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| 2026-02-16 | D-001       | W-007, W-008        | Memory learning is tiered and scoped (`session/project/team/curated`) with explicit promotion; no unrestricted global recall by default. | Prevent cross-project/corporate leakage while preserving cross-session learning.            | Capability gating and promotion pipeline are mandatory for non-session tiers.                                                                 |
| 2026-02-16 | D-002       | W-009, W-010, W-011 | Routines are user-creatable first-class controls; AI may only propose drafts for user activation.                                        | Product requirement for user ownership and safe automation.                                 | Routine UX and policy layers must support user + future connector workflows.                                                                  |
| 2026-02-16 | D-003       | W-016               | Canonical platform term is `orchestrator`; non-committed concepts are tracked separately from implementation specs.                      | Reduce drift/confusion from informal naming and stale aspirational sections in active docs. | `docs/MULTI_AGENTS.md` remains archival; roadmap ideas live in `docs/design/ORCHESTRATOR_ROADMAP.md`.                                         |
| 2026-02-16 | D-004       | W-019               | Mission and routine event families are promoted to stable SDK contracts after snapshot + Desktop/TUI parity verification.                | External builders need reliable event compatibility guarantees across clients.              | `SDK_CONTRACTS.md` marks `mission.created`, `mission.updated`, `routine.fired`, `routine.approval_required`, and `routine.blocked` as stable. |

## Rule

- Any architectural or policy-changing decision must be logged here before or with implementation.
