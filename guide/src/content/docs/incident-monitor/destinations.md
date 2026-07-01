---
title: Incident Monitor Destinations
description: Track destination adapters for governed Incident Monitor publishing.
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

Status: implemented.

Linear creates or reuses issues through configured Linear MCP capabilities. It includes evidence references and triage context, performs duplicate matching, and records Linear issue IDs/URLs in destination-neutral post receipts.

## Signed webhook destination

Status: implemented.

Webhook publishing validates URLs, enforces optional host allowlists, signs payloads with Tandem HMAC SHA-256 headers, bounds payload and response sizes, caps retry attempts, and records durable receipts with delivery ID, status code, attempt metadata, route metadata, and evidence digest. Report-only intake credentials cannot mutate routes or destinations.

## Local telemetry destination

Status: implemented.

Local telemetry publishing records durable destination-aware post receipts that can be filtered by destination ID. Use it when a deployment needs local queryable evidence without requiring an external issue tracker or webhook receiver.

## Generic MCP tool destination

Status: implemented and high risk.

Generic MCP publishing is allowlisted, schema-mapped, route-aware, and disabled by default. It requires explicit admin/full-token configuration with `allow_publish` because MCP tools may mutate external systems.

## Internal memory destination

Status: implemented.

Internal memory stores bounded, redacted incident summaries for recurrence patterns, policy gaps, duplicate failures, and operational risk learning. It records memory refs and duplicate-suppression details in destination-aware receipts.

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
