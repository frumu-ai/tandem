# TANDEM SDK Integration Audit Report

Date: 2026-02-14
Scope: post-integration audit for Session-linear execution contract in server, desktop (Tauri), and CLI (`tandem-tui`).

## Verdict

PASS (follow-ups completed).

No P0/P1 issues were found. The implemented behavior is aligned with `docs/TANDEM_SDK_VISION.md` for the locked runtime decisions.

## Findings (Ordered by Severity)

### Medium

None.

### Low

None.

## What Was Accomplished

1. Session-linear execution core contract implemented on server.

- Added run registry and canonical server-issued `runID` lifecycle.
- Added stale-run reaping with `TANDEM_RUN_STALE_MS` (default 120000, clamped 30000..600000).
- Added run lifecycle events:
  - `session.run.started`
  - `session.run.finished`
  - `session.run.conflict`

2. API surface aligned with SDK wedge.

- Added/confirmed endpoints:
  - `POST /session/{id}/prompt_async`
  - `POST /session/{id}/prompt_sync`
  - `GET /session/{id}/run`
  - `POST /session/{id}/cancel`
  - `POST /session/{id}/run/{run_id}/cancel`
  - `GET /event`
- `prompt_async` contract:
  - default `204 + x-tandem-run-id`
  - `?return=run` => `202` with `{ runID, attachEventStream }`
- Conflict contract:
  - `409` with nested `activeRun` shape and `retryAfterMs`, `attachEventStream`.

3. Message/Execution split enforced.

- `POST /session/{id}/message` is now append-only and always allowed.
- Run lock enforcement applies to `prompt_*` endpoints.

4. Desktop and CLI migrated.

- Desktop/Tauri sidecar flow uses append then async run (`/message` then `/prompt_async?return=run`).
- TUI (`tandem-tui`, CLI) uses append then `prompt_sync` with SSE streaming support and fallback behavior.
- Stream event models extended to include run lifecycle event types.

5. Regression coverage increased.

- Added server tests for:
  - `?return=run` returns usable `runID` and attach stream.
  - `GET /session/{id}/run` active metadata during in-flight run.
  - concurrent same-session `prompt_async` produces `409` with nested `activeRun`.
  - append during active run succeeds.

## Validation Evidence

Commands executed:

- `cargo test -p tandem-server` -> PASS (`18 passed`)
- `cargo check -p tandem` -> PASS
- `cargo check -p tandem-tui` -> PASS
- `cargo check -p tandem -p tandem-tui` -> PASS

Additional closure checks:

- `cargo test -p tandem` -> PASS
- `cargo test -p tandem-tui` -> PASS

## Contract Alignment Checklist

- `/message` append-only: PASS
- Lock only on `prompt_*`: PASS
- `runID` canonical server primitive: PASS
- `GET /session/{id}/run`: PASS
- `?return=run` escape hatch: PASS
- stale-run reaping: PASS
- `prompt_sync` SSE behavior: PASS
- `409` nested `activeRun`: PASS

## Residual Risks

Resolved in this pass:

1. Hard rename completed:

- server handler renamed to `post_session_message_append`
- Tauri command renamed to `send_message_and_start_run`
- frontend TS API renamed to `sendMessageAndStartRun`
- sidecar API renamed to `append_message_and_start_run(_with_context)`

2. Desktop recovery/conflict tests added:

- `recover_active_run_attach_stream_uses_get_run_endpoint`
- `test_parse_prompt_async_response_409_includes_retry_and_attach`
- `test_parse_prompt_async_response_202_parses_run_payload`

3. Cancel-by-runID client tests added:

- desktop sidecar:
  - `cancel_run_by_id_posts_expected_endpoint`
  - `cancel_run_by_id_handles_non_active_run`
- CLI (`tandem-tui`):
  - `cancel_run_by_id_posts_expected_endpoint`
  - `cancel_run_by_id_returns_false_for_non_active_run`
