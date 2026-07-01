---
title: Incident Monitor Overview
description: Understand Incident Monitor's governed intake, routing, evidence, and destination model.
---

Incident Monitor is Tandem's governed incident intake, triage, routing, and evidence layer.

Tandem can ingest failures, create governed incidents and drafts, run triage, require approval, and publish through configured destinations. The destination router keeps GitHub compatibility while adding the same governed route, readiness, receipt, and evidence model for Linear, webhook, telemetry, MCP tool, and internal memory destinations.

## Current behavior

- Incident Monitor remains the production path for failure intake, draft review, triage, approval, and governed destination publishing.
- Legacy configs without explicit destinations synthesize a default `legacy-github` destination.
- GitHub publish still uses the existing MCP capability resolution and duplicate matching behavior.
- Scoped intake keys can report only. They cannot publish, mutate routes or destinations, call tools, inspect files, or bypass approval.
- The authority inventory endpoint summarizes governed runtime, tool, route, destination, source, approval, and publish surfaces for security posture assessment without exposing raw credentials.

## Target flow

```text
signal -> source identity -> incident -> draft -> triage/safety assessment -> route -> destination -> receipt/export
```

The important shift is that Tandem separates the monitored source from the publishing destination. A source can be Tandem itself, an external app, CI, an agent runtime, an MCP gateway, or a customer system. A destination can be GitHub, Linear, signed webhook, local telemetry, generic MCP tool, or internal memory when readiness and policy allow it.

## What agents should know

- Do not assume every incident becomes a GitHub issue.
- Use route preview before publishing when destination choice matters.
- Treat source identity, route tags, allowed destinations, tenant/workspace context, approval policy, and readiness as part of the incident state.
- Do not use scoped intake credentials for publish, route management, destination setup, tool calls, or file inspection.
- Start with [Agent Runtime Guide](../agent-runtime-guide/) when an MCP-connected agent needs to use Incident Monitor safely.

## Implemented now vs deployment policy

Implemented now:

- destination-neutral config, route, readiness, and receipt fields
- `legacy-github` fallback for old configs
- route preview for destination matching and readiness explanation
- normalized external monitored sources and source-level destination binding
- GitHub destination parity through the destination router
- Linear issue destination
- signed webhook destination
- local telemetry destination
- generic MCP tool destination
- internal memory destination
- safety/risk schema expansion
- security-readiness audit coverage
- authority inventory for security posture assessment
- posture rules and finding generation
- controlled dry-run probes for Tandem governance controls
- security gap assessment reports with redacted evidence packs
- deployment cards for production authority governance
- production governance guidance that maps deployment cards, posture checks, assessment reports, protected audit evidence, and destination receipts to operator-owned policy decisions

Deployment-specific policy still matters:

- customer-owned retention and export destinations for reports, receipts, and protected audit evidence
- customer policy mapping for which findings require escalation
- explicit approval and redaction rules for sensitive external destinations
- incident response playbooks for escalation, customer notification, and regulator reporting where applicable

## Related

- [Destination Router](../destination-router/)
- [Agent Runtime Guide](../agent-runtime-guide/)
- [Production Governance](../production-governance/)
- [External Sources](../external-sources/)
- [Destinations](../destinations/)
- [Security Posture Mode](../security-posture/)
- [Setup Checklist](../setup-checklist/)
- [Incident Monitor External Log Intake](../../incident-monitor-external-log-intake/)
