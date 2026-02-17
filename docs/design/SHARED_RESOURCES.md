# Shared Resources (Blackboard)

## Summary

Shared resources provide durable, revisioned coordination across agents and clients.

## Namespaces

- `run/*`
- `mission/*`
- `project/*`
- `team/*`

## Resource Model

- `key`: hierarchical namespace key
- `value`: JSON document
- `rev`: monotonic revision
- `updated_at`, `updated_by`
- optional `ttl_ms`

## Concurrency

- Optimistic concurrency via `if_match_rev`.
- Lease-assisted lock ownership can reuse `/global/lease/*`.

## API Surface

- `GET /resource?prefix=...`
- `GET /resource/{key}`
- `PUT /resource/{key}`
- `PATCH /resource/{key}`
- `GET /resource/events?prefix=...` (SSE)

## Eventing

- Emit `resource.updated` and `resource.deleted` as `EngineEvent`.
- Prefix filters must only deliver matching keys.
