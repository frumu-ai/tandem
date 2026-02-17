# Memory Tiers and Governance

## Summary

Cross-session learning is enabled by scoped tiers and explicit promotion, not global unrestricted recall.

## Tiers

- `session`: default write scope, ephemeral, no leakage by default.
- `project`: persistent per repo/workspace, default read companion to session.
- `team`: shared LAN/team memory, opt-in and capability-gated.
- `curated`: reviewed golden patterns safe for controlled auto-use.

## Hard Partitioning

All operations are partitioned by:
`{org_id}/{workspace_id}/{project_id}/{tier}`

This prevents cross-project/team leakage unless capability claims explicitly permit it.

## Capability Model

Per run token fields:

- readable tiers
- writable tiers
- promotable target tiers
- review requirement for promotion
- auto-use allowed tiers

Fail-safe defaults:

- read: `session`, `project`
- write: `session`
- promote: none
- auto-use: `curated` only

## Promotion Pipeline

1. Write raw context to `session`.
2. On done/review/test gate, build sanitized solution capsule.
3. Run scrubber (secrets/PII/sensitive markers).
4. Require reviewer/policy approval for promotion.
5. Promote to `project`, `team`, or `curated` with audit receipt.

## API Contracts

- `POST /memory/put`
- `POST /memory/promote`
- `POST /memory/search`
- `GET /memory/audit`

Reference Rust contract types:
`tandem/crates/tandem-memory/src/governance.rs`

## Trust Center UI Cues

- Active memory scopes.
- Capability badges (read/write/promote).
- Risk badge when team recall is enabled.
- Promotion panel with scrub report and reviewer identity.
- Audit link for every promoted capsule.
