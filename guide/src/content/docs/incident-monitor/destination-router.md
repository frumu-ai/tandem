---
title: Incident Monitor Destination Router
description: Route Incident Monitor drafts to governed destinations with preview, readiness, approval, and receipts.
---

The destination router decides where an incident draft can be published. It is deliberately separate from intake and triage so Tandem can explain route decisions before mutating any external system.

## Router concepts

- `destinations` describe publish targets such as GitHub, Linear, webhook, telemetry, MCP tool, or internal memory.
- `routes` match incident context to destination IDs.
- `default_destination_ids` are used when no route matches.
- `allowed_destination_ids` on a monitored project or log source restrict where that source can publish.
- `route_tags` provide low-cardinality routing hints from sources, submissions, drafts, and incidents.
- readiness checks explain whether the selected destination can publish.
- receipts record the destination, external ID, external URL, route, operation, status, and evidence digest.

## Route preview

Route preview is the safe read path for publish planning. It should answer:

- which routes matched
- which destination IDs are effective
- whether approval is required
- whether a destination is blocked or unready
- whether a source allowlist prevents a destination

Preview does not publish, call destination tools, or mutate external systems.

## Approval and fail-closed behavior

Publishing can be blocked by global config, destination readiness, route approval policy, source approval policy, or high-risk safety defaults.

High-risk incidents should require approval unless a trusted route/source policy explicitly says otherwise. Untrusted report-only submissions cannot downgrade approval policy.

Destination readiness is enforced fail-closed by default. `block_unready_destinations` defaults to `true`, and readiness is checked again at delivery time — after any required approval clears — so an unready destination cannot publish even if a route or approval decision looked clean at preview time. A blocked delivery returns an error with the readiness detail, emits a `destination_not_ready` audit event, and produces no receipt.

The escape hatch is deliberately narrow. Setting `block_unready_destinations` to `false` only frees `Recovery`-mode publishes (best-effort recovery paths); `Auto` and `ManualPublish` always block unready destinations regardless of the flag. `RecheckOnly` never publishes, so it is never gated. Preview still surfaces readiness problems ahead of any publish attempt.

## GitHub compatibility

Legacy Incident Monitor configs synthesize:

```json
{
  "destination_id": "legacy-github",
  "kind": "github_issue"
}
```

Configured GitHub destinations use the same publisher adapter but carry their configured destination ID in posts, receipts, route metadata, and external-action mirrors.

## Webhook destinations

Signed webhook destinations publish bounded JSON incident payloads to configured HTTP endpoints. They require an env-backed `webhook_secret_ref`, sign requests with Tandem HMAC SHA-256 headers, block localhost/private/internal URL ranges by default, support optional host allowlists, and record per-delivery receipts with status code, attempt count, delivery ID, route metadata, and evidence digest.

## Agent guidance

- Prefer route preview before changing publish behavior.
- Keep duplicate checks destination-aware.
- Preserve legacy `/incident-monitor/drafts/{id}/publish` response fields.
- Never let a scoped intake key choose arbitrary destinations or invoke destination tools.
