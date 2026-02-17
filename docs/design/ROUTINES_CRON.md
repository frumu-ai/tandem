# Routines (Internal Cron)

## Summary

Routines are persisted scheduler jobs that trigger mission/run entrypoints with caps and lease-safe execution.
Routines must be creatable by end users through product controls, not only by AI-generated flows.

## RoutineSpec

- schedule: cron or interval
- timezone
- misfire policy: `skip | run_once | catch_up(n)`
- caps and budgets
- mission entrypoint + args
- creator metadata (`creator_type=user|agent`, `creator_id`)
- execution policy (`requires_approval`, `external_integrations_allowed`)

## Runtime Rules

- Persist `next_fire_at`.
- On fire, acquire lease to prevent duplicate execution.
- Create mission/run with constrained capabilities and budgets.
- Emit routine lifecycle events to `EngineEvent`.
- If routine uses external integrations (future connectors), enforce explicit capability checks and approval policy before side effects.

## Authoring Modes

- User-authored routine: created/edited via Desktop/TUI controls, owned by user identity.
- Agent-authored draft: AI may propose routine configs, but activation requires explicit user confirmation.
- Shared template routine: installable preset that users can review and customize before enabling.

## External Services (Future)

Routines are designed to support future connector tasks such as:

- read emails and draft replies
- generate social posts on schedule
- periodic research and synthesis

For connector-backed routines, default posture is safe:

- dry-run/suggest mode by default
- approval required for outbound side effects unless user policy explicitly allows auto-execution

## API Surface

- `POST /routines`
- `GET /routines`
- `PATCH /routines/{id}`
- `DELETE /routines/{id}`
- `POST /routines/{id}/run_now`
- `GET /routines/events` (SSE)
- `GET /routines/{id}/history`

## UI Controls (Required)

- Create/edit/pause/delete routine from Desktop and TUI.
- "Run now" manual trigger from UI.
- Visibility into next run time, last result, and pending approvals.
- Clear badge when routine includes external side effects.
