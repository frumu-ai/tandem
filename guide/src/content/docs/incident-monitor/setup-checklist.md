---
title: Incident Monitor Setup Checklist
description: Configure Incident Monitor intake, routing, destinations, security posture, and production governance evidence.
---

Use this checklist when setting up Incident Monitor intake, destination routing, security posture, and production governance evidence.

## Current GitHub setup

1. Enable Incident Monitor in `Settings -> Incident Monitor`.
2. Set the GitHub repo in `owner/repo` format.
3. Select an MCP server with GitHub list, get, create, and comment capabilities.
4. Keep `require_approval_for_new_issues` aligned with team policy.
5. Run status/readiness and confirm GitHub read/write capabilities are available.
6. Submit a manual report or external log fixture.
7. Create or refresh the triage run.
8. Preview or publish only after the draft has enough evidence.
9. Confirm the post receipt includes the destination ID, external URL, and evidence digest.

## External source setup

1. Add `monitored_projects` with stable `project_id`, `repo`, and `workspace_root`.
2. Add `log_sources` with stable `source_id` values.
3. Set `source_kind`, route tags, tenant/workspace IDs, and schema version where known.
4. Set `allowed_destination_ids` and `default_destination_ids` for any source that must be constrained.
5. Create scoped intake keys only for report-only external systems.
6. Confirm scoped keys cannot publish, mutate routes/destinations, call tools, or inspect files.

## Destination-router setup

1. Define destinations explicitly when moving beyond the legacy fallback.
2. Add routes only when default destinations are insufficient.
3. Use route preview to confirm matches, readiness, approval, and blocked reasons.
4. Enable Linear, webhook, telemetry, memory, and MCP destinations only after their readiness and receipt behavior match deployment policy.
5. Treat generic MCP destinations as high risk until `allow_publish`, server/tool allowlists, payload mapping, approval policy, and redaction are reviewed.
6. Preserve `legacy-github` behavior for old configs.

## Security posture preparation

1. Inventory agents, workflows, tools, sources, destinations, approvals, and tenant/workspace context.
2. Add deterministic checks before adding controlled probes.
3. Make probes authorized, bounded, and dry-run or sandboxed where possible.
4. Export evidence to a customer-owned audit destination when assessing Tandem itself.

## Production governance preparation

1. Generate deployment cards for Tandem self-monitoring, monitored sources, high-authority agents, and externally mutating workflows.
2. Fill owner, accountable team, intended purpose, data classification, approval protocol, escalation protocol, and review cadence metadata.
3. Confirm reports, receipts, and protected audit evidence have a customer-owned retention/export policy before production use.
4. Map posture findings to customer policy and assign owners before enabling high-risk external destinations.
5. Use [Production Governance](./production-governance/) for the full operating-model checklist.
