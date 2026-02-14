# TANDEM SDK Vision

This document has two explicit layers:

- **Layer A: Vision + Decisions** (product-readable).
- **Layer B: Runtime Contract (Normative)** (implementation-ready).

Current runtime reality: Tandem is HTTP + SSE today (engine server endpoints + event stream).  
Stdio JSON-RPC is a later roadmap item, not a current contract.

## Layer A: Vision + Decisions

## Summary

Audience: **external builders** integrating local-first AI workflows into desktop apps, CLIs, IDE extensions, and automation pipelines.

Tandem runtime positioning: **Session-linear execution** on a **Linearizable session runtime**.

Locked decisions:

- `POST /session/{id}/message` is append-only.
- Run lock applies only to `prompt_*` execution endpoints.
- `runID` is server-issued and canonical.
- `GET /session/{id}/run` is required.
- `?return=run` escape hatch is required.
- Stale-run reaping is required.
- `prompt_sync` supports `Accept: text/event-stream`.
- `409` conflict payload uses nested `activeRun` (not flattened).

## Message vs Execution Split

- `POST /session/{id}/message`:
  - append-only
  - always allowed
  - persists messages only
- `POST /session/{id}/prompt_async`:
  - execution start
  - lock-gated
- `POST /session/{id}/prompt_sync`:
  - execution start
  - lock-gated

Desktop/Tauri intended flow:

- `/message` then `/prompt_async`
- Not "run via `/message`"

## Concurrency Model: Many Sessions = Many Agents

- A single session is intentionally linearizable and represents one active-run boundary.
- Parallel prompts inside one session are explicitly out of contract.
- Parallelism is achieved through multiple sessions (often child sessions) and aggregation.

## Migration Plan

Desktop/Tauri:

- Remove run-via-`/message` assumptions.
- Use `runID` from `?return=run` response or `GET /session/{id}/run`.
- After a reconnect, the preferred recovery primitive is `GET /session/{id}/run`, then attach via `attachEventStream` when active.
- On `409`, use `retryAfterMs` and `attachEventStream`.

TUI:

- Migrate from run-via-`/message` to `prompt_sync` (preferred, can be streaming).
- Alternative path: `prompt_async + /event` attach.

## OpenAPI Wedge Scope

The wedge contract freezes these endpoints:

- `/global/health`
- `/session` create/list/get
- `/session/{id}/message`
- `/session/{id}/prompt_async`
- `/session/{id}/prompt_sync`
- `/session/{id}/run`
- `/session/{id}/cancel`
- `/session/{id}/run/{run_id}/cancel`
- `/event`
- `/provider`

## Test Cases

- Append during active run via `/message` succeeds.
- Concurrent same-session `prompt_async` => one success + one `409`.
- `?return=run` => `202` with usable `runID`.
- `GET /session/{id}/run` supports reconnect/recovery.
- stale-run timeout clears ghost lock and emits `session.run.finished(status=timeout)`.
- `prompt_sync` SSE mode streams incrementally.
- `/event` server-side filtering reduces non-matching stream traffic.
- cancel by session and by `runID` both work.

## Not in scope today

- Stdio JSON-RPC adapter (roadmap item).
- Full OpenAPI surface beyond the wedge endpoints.

# Appendix: Runtime Contract (Normative)

## Async Start Contract

- `POST /session/{id}/prompt_async` default response:
  - `204 No Content`
  - `x-tandem-run-id` response header
- `POST /session/{id}/prompt_async?return=run` response:
  - `202 Accepted`
  - JSON body containing:
    - `runID`
    - `attachEventStream`

## Conflict Contract

Canonical `409` conflict schema:

```json
{
  "code": "SESSION_RUN_CONFLICT",
  "sessionID": "...",
  "activeRun": {
    "runID": "...",
    "startedAtMs": 0,
    "lastActivityAtMs": 0,
    "clientID": null
  },
  "retryAfterMs": 500,
  "attachEventStream": "/event?sessionID=...&runID=..."
}
```

- `retryAfterMs` is a backoff hint.
- `attachEventStream` is the immediate observe/recovery path.

## Run Discovery Contract

`GET /session/{id}/run` returns either:

- `{ "active": null }`
- `{ "active": { ...run metadata... } }`

## Stale-Run Reaping Contract

- `STALE_MS` default: `120000`
- Env var: `TANDEM_RUN_STALE_MS`
- Clamp range: `30000..600000`
- If stale:
  - auto-release lock
  - emit `session.run.finished` with `status=timeout`

## Run Event Shapes

- `session.run.started`:
  - `sessionID`
  - `runID`
  - `startedAtMs`
  - optional `clientID`
- `session.run.finished`:
  - `sessionID`
  - `runID`
  - `finishedAtMs`
  - `status=completed|cancelled|error|timeout`
  - Example status values: completed, cancelled, error, timeout.
  - optional `error`
- `session.run.conflict`:
  - `sessionID`
  - active `runID`
  - `retryAfterMs`
  - `attachEventStream`

## /event Filtering Semantics

- Filtering is server-side before writing events to the response stream.
- Query params:
  - required filter option: `sessionID`
  - optional: `runID`

## prompt_sync Streaming Contract

- If `Accept: text/event-stream`, stream incremental run/content/tool events.
- If non-SSE `Accept`, return final JSON after completion.
- Same lock rules as `prompt_async`.
