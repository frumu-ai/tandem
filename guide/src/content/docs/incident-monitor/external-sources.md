---
title: Incident Monitor External Sources
description: Model monitored systems outside Tandem with explicit identity, route tags, and destination limits.
---

External sources tell Tandem what it is monitoring when the event did not originate inside Tandem itself.

## Source identity

A monitored source should have stable identifiers:

- `project_id`
- `log_source_id` when the event came from a watched file
- `source_kind`, such as `external_app`, `ci`, `agent_runtime`, `mcp_gateway`, or `customer_system`
- `tenant_id` and `workspace_id` when available
- `event_schema_version` for externally reported payloads
- default `route_tags`

This identity lets the router distinguish "Tandem observed a customer CI failure" from "Tandem itself failed".

## Destination restrictions

Projects and log sources can set:

- `allowed_destination_ids`
- `default_destination_ids`
- `approval_policy`
- redaction and retention profiles

If both project and source allowlists exist, the effective allowlist is their intersection. If a source tries to publish outside that set, route preview and publish should fail closed.

## Scoped intake keys

Scoped intake keys are report-only credentials. They can submit events for their configured project and scope, but they cannot:

- publish incidents
- mutate routes or destinations
- call MCP tools
- inspect files
- change approvals
- read unrelated incidents, drafts, or logs

## Existing external log intake

Bug Monitor already supports local external log intake for configured projects. Use [Bug Monitor External Log Intake](../bug-monitor-external-log-intake/) for current setup details.

Incident Monitor builds on that source model so future destination routing can use the same source identity and allowlists.
