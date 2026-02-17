# SDK Contracts (Phase 6)

## Purpose

Track third-party builder contracts and compatibility obligations for mission/resource/routine integrations.

## Stability Policy

- `stable`: safe for external client dependency; changes require compatibility strategy.
- `provisional`: available but may evolve before being declared stable.

## HTTP Contracts

| Contract                                                                                                                       | Stability | Owner                            | Verification                                                    |
| ------------------------------------------------------------------------------------------------------------------------------ | --------- | -------------------------------- | --------------------------------------------------------------- |
| Mission API (`POST /mission`, `GET /mission`, `GET /mission/{id}`, `POST /mission/{id}/event`)                                 | stable    | `tandem-server`                  | Mission API tests in server + desktop/tui client contract tests |
| Resource API (`GET/PUT/PATCH/DELETE /resource/{*key}`, `GET /resource?prefix=...`)                                             | stable    | `tandem-server`                  | Resource HTTP + event shape tests                               |
| Memory API (`POST /memory/put`, `POST /memory/promote`, `POST /memory/search`, `GET /memory/audit`)                            | stable    | `tandem-server`, `tandem-memory` | Governance and promotion scrub/audit tests                      |
| Routine API (`POST/GET /routines`, `PATCH/DELETE /routines/{id}`, `POST /routines/{id}/run_now`, `GET /routines/{id}/history`) | stable    | `tandem-server`                  | Routine scheduler + policy + history tests                      |

## Event Contracts

| Event Family                                                                        | Stability | Owner                                  | Verification                                                                        |
| ----------------------------------------------------------------------------------- | --------- | -------------------------------------- | ----------------------------------------------------------------------------------- |
| `resource.updated`, `resource.deleted`                                              | stable    | `tandem-server`                        | Event payload snapshot tests                                                        |
| Mission lifecycle (`mission.created`, `mission.updated`) emitted via `EngineEvent`  | stable    | `tandem-orchestrator`, `tandem-server` | Mission event contract snapshot tests + Desktop/TUI event payload consumption tests |
| Routine lifecycle (`routine.fired`, `routine.approval_required`, `routine.blocked`) | stable    | `tandem-server`                        | Routine event contract snapshot tests + Desktop/TUI event payload consumption tests |

## Client Parity Requirement

Desktop (`tandem/src-tauri`) and TUI (`tandem/crates/tandem-tui`) must expose equivalent behavior for all stable contracts.

## W-019 Hardening Completion

- Added mission lifecycle event payload snapshot tests (server side) for reducer-driven transitions.
- Added routine lifecycle event payload snapshot tests for `routine.fired`, `routine.approval_required`, and `routine.blocked`.
- Added Desktop and TUI parity tests that consume mission/routine event payloads without adapter drift.
- Promoted mission/routine event families to `stable` after snapshot + parity checks passed.

## Change Control

1. If a stable contract changes, update this file and `DECISIONS.md`.
2. Add/adjust acceptance criteria in `WORKBOARD.md`.
3. Add a `PROGRESS_LOG.md` entry with verification commands.
