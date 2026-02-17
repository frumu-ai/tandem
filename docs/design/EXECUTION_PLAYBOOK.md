# Execution Playbook

## Goal

Coordinate a large multi-file build without losing scope, status, or ownership.

## Build Flow

1. Pick next `todo` item from `WORKBOARD.md`.
2. Move it to `in_progress`.
3. Implement only that item's acceptance criteria.
4. Verify locally (targeted checks/tests).
5. Update `PROGRESS_LOG.md` with result.
6. If policy/architecture changed, add `DECISIONS.md` entry.
7. Move item to `done` only after verification.

## Branch and Commit Conventions

- Branch naming: `feat/W-007-memory-governance-api`
- Commit message prefix: `[W-007] short description`
- PR title prefix: `[W-007] short description`

## Push Slicing Rules

- One PR should primarily close one work item.
- If a PR spans multiple items, all IDs must be listed in title and summary.
- Avoid mixing unrelated phases in the same PR.

## Definition of Done

- Acceptance criteria in `WORKBOARD.md` is met.
- Relevant tests/checks pass.
- `PROGRESS_LOG.md` updated.
- Any new decision logged in `DECISIONS.md`.
- Any changed API reflected in the corresponding design spec.

## Anti-Drift Rules

- Do not implement from chat memory; implement from `WORKBOARD.md`.
- Do not mark `done` without explicit verification command/output.
- Do not create orphan docs without linking them from `README.md` and `IMPLEMENTATION_PLAN.md`.
