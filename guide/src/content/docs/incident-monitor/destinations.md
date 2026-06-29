---
title: Incident Monitor Destinations
description: Track current and planned destination adapters for Incident Monitor publishing.
---

Destinations are governed publish targets. Every destination should have explicit readiness, route behavior, approval policy, idempotency, and receipt semantics.

## GitHub issue destination

Status: implemented as the current production destination.

GitHub supports:

- issue creation
- comment on matched open issue
- hidden fingerprint and evidence markers
- duplicate matching
- unsafe create retry suppression
- destination-aware post receipts
- external-action mirror receipts

Legacy configs use the synthesized `legacy-github` destination. Explicit GitHub destinations can carry their own destination ID while using the same GitHub publisher adapter.

## Linear issue destination

Status: planned.

Linear should create or reuse issues through configured Linear capability resolution. It should map incident severity and risk to Linear priority, include evidence references and triage context, and record Linear issue IDs/URLs in destination-neutral post receipts.

## Signed webhook destination

Status: planned.

Webhook publishing should validate URLs, enforce allowlists, sign payloads with HMAC, bound payload size, retry safely, and record durable receipts. It should never be selectable by report-only intake credentials.

## Telemetry/database destination

Status: planned.

Telemetry/database publishing should write local, queryable incident payloads and receipts without requiring an external service.

## Generic MCP tool destination

Status: planned and high risk.

Generic MCP publishing must be allowlisted, schema-mapped, route-aware, and disabled by default. It should require explicit admin/full-token configuration because MCP tools may mutate external systems.

## Internal memory destination

Status: planned.

Internal memory should store bounded, redacted incident summaries for recurrence patterns, policy gaps, duplicate failures, and operational risk learning.

## Receipt requirements

Every destination should record:

- destination ID and kind
- route ID and match reason when available
- operation and status
- external ID, URL, and title when available
- target reference
- evidence digest
- error or response excerpt
- created and updated timestamps
