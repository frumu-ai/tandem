# Implementation Plan

## Coordination

- Execution hub: `README.md`
- Active queue: `WORKBOARD.md`
- Progress trail: `PROGRESS_LOG.md`
- Decision trail: `DECISIONS.md`
- Build workflow: `EXECUTION_PLAYBOOK.md`

## Phase 1

- Add shared resource store + revisioned API + SSE.
- Add status indexer from run/tool events.
- Stabilize resource event schema.

## Phase 2

- Extract shared orchestrator crate and reducer abstraction.
- Add mission/work item model and default mission flow.
- Stabilize mission event schema.

## Phase 3

- Add reviewer/tester gates and artifact linking.
- Add explicit approval checkpoints for sensitive actions.

## Phase 4

- Expand memory governance with team/curated contracts and promotion pipeline.
- Enforce capability token claims + scrub/audit.

## Phase 5

- Add routines scheduler with lease-safe execution and misfire handling.
- Add end-user routine controls in Desktop/TUI (create/edit/pause/run-now/delete).
- Add policy gates for future external-service routines (approval + capability checks).

## Phase 6

- Desktop/TUI control center parity and SDK ergonomics.
- W-017 focus: operator UX refinement backlog for mission/routine command-center flows.
- W-018 focus: SDK contract hardening and builder-facing compatibility guarantees.

## Stabilized Contracts for Builders

- `EngineEvent` envelopes for mission/resource/routine families.
- Orchestrator run/task APIs.
- Resource APIs (`GET/PUT/PATCH/events`).
- Memory governance APIs (`put/promote/search`).
- Canonical matrix in `SDK_CONTRACTS.md` (stability + ownership + verification).

## Acceptance Gates

- Contract snapshot tests for event payload shape.
- Concurrency tests for revision conflicts and lease ownership.
- Isolation tests for cross-project memory access.
- Promotion scrub/audit tests.
- Routine authorization tests: user-created vs agent-proposed activation flows.
- External side-effect policy tests for connector-backed routine tasks.
- Phase 6 operator checks: Desktop and TUI both support mission create/list/get/event and routine create/edit/pause/run-now/delete/history workflows with matching outcomes.
- Phase 6 SDK checks: published examples validate mission/resource/routine HTTP and SSE flows against current server contracts.
