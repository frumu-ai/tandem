---
title: Incident Monitor Overview
description: Understand how Bug Monitor evolves into destination-agnostic Incident Monitor while preserving GitHub compatibility.
---

Incident Monitor is the destination-agnostic evolution of Bug Monitor.

Today, Tandem can ingest failures, create governed incidents and drafts, run triage, require approval, and publish to GitHub. The destination-router work keeps that GitHub behavior compatible while adding the model needed for Linear, webhook, telemetry/database, MCP tool, and internal memory destinations.

## Current behavior

- Bug Monitor remains the production path for failure intake, draft review, triage, approval, and GitHub issue/comment publishing.
- Legacy configs without explicit destinations synthesize a default `legacy-github` destination.
- GitHub publish still uses the existing MCP capability resolution and duplicate matching behavior.
- Scoped intake keys can report only. They cannot publish, mutate routes or destinations, call tools, inspect files, or bypass approval.
- The authority inventory endpoint summarizes governed runtime, tool, route, destination, source, approval, and publish surfaces for security posture assessment without exposing raw credentials.

## Target flow

```text
signal -> source identity -> incident -> draft -> triage/safety assessment -> route -> destination -> receipt/export
```

The important shift is that Tandem separates the monitored source from the publishing destination. A source can be Tandem itself, an external app, CI, an agent runtime, an MCP gateway, or a customer system. A destination can be GitHub today and other governed destinations later.

## What agents should know

- Do not assume every incident becomes a GitHub issue.
- Use route preview before publishing when destination choice matters.
- Treat source identity, route tags, allowed destinations, tenant/workspace context, approval policy, and readiness as part of the incident state.
- Preserve GitHub compatibility when touching current Bug Monitor paths.
- Do not use scoped intake credentials for publish, route management, destination setup, tool calls, or file inspection.

## Implemented now vs planned

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

Planned:

- posture rules and finding generation
- controlled dry-run probes for governance controls
- evidence report export packs
- external audit export for self-monitoring evidence

## Related

- [Destination Router](./destination-router/)
- [External Sources](./external-sources/)
- [Destinations](./destinations/)
- [Security Posture Mode](./security-posture/)
- [Setup Checklist](./setup-checklist/)
- [Bug Monitor External Log Intake](../bug-monitor-external-log-intake/)
