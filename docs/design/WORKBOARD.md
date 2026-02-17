# Workboard

## Status Key

- `todo`: not started
- `in_progress`: currently being executed
- `blocked`: waiting on dependency/decision
- `done`: implemented and verified

## Work Items

| ID    | Phase | Title                                                                                     | Source Spec               | Owner | Status | Acceptance                                                                                                         |
| ----- | ----- | ----------------------------------------------------------------------------------------- | ------------------------- | ----- | ------ | ------------------------------------------------------------------------------------------------------------------ |
| W-001 | 1     | Shared resources store schema + persistence                                               | `SHARED_RESOURCES.md`     | Codex | done   | Resource keys support rev + optimistic concurrency                                                                 |
| W-002 | 1     | Resource HTTP endpoints (`GET/PUT/PATCH/events`)                                          | `SHARED_RESOURCES.md`     | Codex | done   | API responds and emits SSE events by prefix                                                                        |
| W-003 | 1     | Resource event contract snapshots                                                         | `IMPLEMENTATION_PLAN.md`  | Codex | done   | Snapshot tests for `resource.*` payload shape                                                                      |
| W-013 | 1     | Status indexer from run/tool events into shared resources                                 | `IMPLEMENTATION_PLAN.md`  | Codex | done   | Derive `run/{sessionID}/status` from `session.run.*` and tool `message.part.updated` events                        |
| W-004 | 2     | Extract orchestrator crate scaffold                                                       | `ORCHESTRATOR.md`         | Codex | done   | New crate compiles and desktop adapters build                                                                      |
| W-005 | 2     | Mission reducer core types and trait                                                      | `ORCHESTRATOR.md`         | Codex | done   | `init`/`on_event` interface implemented                                                                            |
| W-014 | 2     | Mission runtime API scaffold in server (`create/get/list/apply event`)                    | `ORCHESTRATOR.md`         | Codex | done   | Mission events apply through reducer and return commands                                                           |
| W-006 | 3     | Reviewer/tester gates in mission flow                                                     | `DEFAULT_MISSION_FLOW.md` | Codex | done   | Gate failures return work item to rework state                                                                     |
| W-007 | 4     | Memory governance API handlers (`put/promote/search`)                                     | `MEMORY_TIERS.md`         | Codex | done   | Endpoints enforce capability + partition checks                                                                    |
| W-008 | 4     | Memory promotion scrub + audit pipeline                                                   | `MEMORY_TIERS.md`         | Codex | done   | Promotion blocked/redacted/passed with audit record                                                                |
| W-009 | 5     | Routine scheduler persistence + misfire policies                                          | `ROUTINES_CRON.md`        | Codex | done   | `skip/run_once/catch_up(n)` verified                                                                               |
| W-015 | 5     | Routine engine API scaffold (`create/list/patch/delete/run-now/history/events`)           | `ROUTINES_CRON.md`        | Codex | done   | Endpoints manage routines and emit/watch lifecycle events                                                          |
| W-010 | 5     | User routine controls (create/edit/pause/run-now/delete)                                  | `ROUTINES_CRON.md`        | Codex | done   | Desktop + TUI controls functional                                                                                  |
| W-011 | 5     | Connector-side-effect policy gates for routines                                           | `ROUTINES_CRON.md`        | Codex | done   | External side effects require policy/approval by default                                                           |
| W-012 | 6     | Desktop + TUI parity wiring to engine-hosted orchestrator APIs                            | `IMPLEMENTATION_PLAN.md`  | Codex | done   | Both clients can observe/control same run                                                                          |
| W-016 | 6     | Phase 6 docs/control-plane reconciliation for orchestrator naming + source-of-truth links | `README.md`               | Codex | done   | Docs cleanly distinguish implemented vs roadmap and link to active specs without stale claims                      |
| W-017 | 6     | Control-center operator UX refinement backlog (Desktop + TUI)                             | `IMPLEMENTATION_PLAN.md`  | Codex | done   | Phase 6 operator workflows and acceptance checks are explicitly defined for both clients                           |
| W-018 | 6     | SDK ergonomics and contract hardening backlog for third-party builders                    | `IMPLEMENTATION_PLAN.md`  | Codex | done   | Remaining stable API/event contracts and contract-test coverage are enumerated with owners                         |
| W-019 | 6     | SDK contract test coverage expansion plan                                                 | `SDK_CONTRACTS.md`        | Codex | done   | Mission/routine event contract families are snapshot-tested, parity-checked in Desktop/TUI, and promoted to stable |

## Backlog Intake Rules

- Add new work item with unique `W-###`.
- Link to exact spec file.
- Define measurable acceptance in one line.
- Do not start implementation until item is `in_progress`.
