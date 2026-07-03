---
title: Incident Monitor Agent Runtime Guide
description: Use Incident Monitor safely from MCP-connected agents, SDK clients, HTTP calls, and public runtime docs.
---

Use this page as the first stop when an agent needs to turn a failure, safety signal, operator finding, or recurring runtime issue into governed Incident Monitor evidence.

Incident Monitor is not a shortcut around Tandem governance. It separates intake, triage, route preview, approval, publishing, and receipts so agents can preserve evidence before anything mutates an external destination.

## Decision Path

| Need                                      | Use                                                                                    | Auth boundary                                      |
| ----------------------------------------- | -------------------------------------------------------------------------------------- | -------------------------------------------------- |
| Check whether Incident Monitor is usable  | `GET /incident-monitor/status` or SDK `getStatus()`                                    | Full engine token                                  |
| Send a manual agent/operator report       | `POST /incident-monitor/report` or SDK `report()`                                      | Full engine token                                  |
| Let CI or an external service report only | `POST /incident-monitor/intake/report`                                                 | Scoped intake key                                  |
| Inspect incidents or drafts               | `GET /incident-monitor/incidents`, `GET /incident-monitor/drafts`, or SDK list helpers | Full engine token                                  |
| See where a draft would publish           | `POST /incident-monitor/route-preview` or SDK `previewRoute()` / `preview_route()`     | Full engine token                                  |
| Add triage evidence                       | `POST /incident-monitor/drafts/{id}/triage-run` or SDK triage helpers                  | Full engine token                                  |
| Approve or deny a draft                   | `POST /incident-monitor/drafts/{id}/approve` or `/deny`                                | Full engine token plus policy permission           |
| Publish after governance checks           | `POST /incident-monitor/drafts/{id}/publish` or SDK publish helpers                    | Full engine token plus route/destination readiness |
| Collect governance evidence               | authority inventory, posture checks, assessment reports, deployment cards              | Full engine token                                  |

When in doubt, inspect first. Reporting creates intake; publishing mutates an external destination.

## Safe Agent Sequence

For MCP-connected agents and autonomous runtime clients, use this sequence:

1. Check readiness with `getStatus()` or `GET /incident-monitor/status`.
2. Identify the source, route tags, tenant/workspace context, and expected destination class.
3. Use route preview before publish when destination choice matters.
4. Report or ingest the incident with the narrowest valid credential.
5. Inspect the draft and run triage before asking to publish.
6. Require approval for high-risk, external, ambiguous, or policy-sensitive drafts.
7. Publish only through Incident Monitor, not by calling GitHub, Linear, webhook, memory, telemetry, or MCP destination tools directly.
8. Confirm the resulting post/receipt includes destination ID, route metadata, status, external URL or ID when available, and evidence digest.

This keeps Tandem as the runtime authority even when the final destination is an MCP tool or another external system.

## Auth Boundaries

| Credential                 | Can do                                                                                                                                                     | Cannot do                                                                                                                            |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Full engine token          | Read status, inspect incidents/drafts/posts, configure routes/destinations, preview routes, run triage, approve/deny, publish, collect governance evidence | Bypass approval, route readiness, destination policy, or audit requirements                                                          |
| Scoped intake key          | Submit report-only intake for its configured project/scope                                                                                                 | Read files, call tools, inspect incidents/drafts, preview routes, mutate config, create keys, reset log offsets, approve, or publish |
| MCP destination capability | Execute only when Incident Monitor publishes through an explicitly configured destination                                                                  | Act as caller auth for an agent, bypass route preview, or replace Tandem approval policy                                             |

Scoped intake keys should usually have only `incident_monitor:report`. Treat them like narrow webhook credentials for incoming evidence, not as general engine credentials.

## Runtime Surfaces

Use the highest-level surface that fits the client:

- Control Panel: `Settings -> Incident Monitor` for setup, sources, destinations, routing, safety defaults, route preview, and readiness.
- TypeScript SDK: `client.incidentMonitor`.
- Python SDK: `client.incident_monitor`.
- HTTP API: `/incident-monitor/*` plus `/config/incident-monitor`.

Compact TypeScript flow:

```typescript
const status = await client.incidentMonitor.getStatus();
const readiness = status.status?.readiness ?? {};
if (
  status.status?.config?.enabled === false ||
  readiness.config_valid === false ||
  readiness.ingest_ready === false ||
  readiness.runtime_ready === false ||
  readiness.route_preview_ready === false
) {
  throw new Error("Incident Monitor is not ready");
}

const preview = await client.incidentMonitor.previewRoute({
  route_tags: ["runtime-failure"],
  risk_category: "tool_policy",
});

if (preview.blocked_reasons?.length) {
  throw new Error(`Route blocked: ${preview.blocked_reasons.join(", ")}`);
}

await client.incidentMonitor.report({
  title: "Agent run failed during Linear sync",
  detail: "The workflow could not resolve the Linear MCP capability.",
  source: "automation_v2",
  event: "automation_v2.run.failed",
  level: "error",
  route_tags: ["runtime-failure"],
});

const drafts = await client.incidentMonitor.listDrafts({ limit: 10 });
if (drafts.drafts[0]) {
  await client.incidentMonitor.createTriageRun(drafts.drafts[0].draft_id);
}
```

Compact Python flow:

```python
status = await client.incident_monitor.get_status()
status_row = status.status
readiness = status_row.readiness or {}
if (
    status_row.config
    and status_row.config.enabled is False
) or any(
    readiness.get(key) is False
    for key in ("config_valid", "ingest_ready", "runtime_ready", "route_preview_ready")
):
    raise RuntimeError("Incident Monitor is not ready")

preview = await client.incident_monitor.preview_route({
    "route_tags": ["runtime-failure"],
    "risk_category": "tool_policy",
})

if preview.blocked_reasons:
    raise RuntimeError(f"Route blocked: {', '.join(preview.blocked_reasons)}")

await client.incident_monitor.report({
    "report": {
        "title": "Agent run failed during Linear sync",
        "detail": "The workflow could not resolve the Linear MCP capability.",
        "source": "automation_v2",
        "event": "automation_v2.run.failed",
        "level": "error",
        "route_tags": ["runtime-failure"],
    },
})

drafts = await client.incident_monitor.list_drafts(limit=10)
if drafts.drafts:
    await client.incident_monitor.create_triage_run(drafts.drafts[0].draft_id)
```

Scoped intake HTTP flow:

```bash
curl -X POST "$TANDEM_BASE_URL/incident-monitor/intake/report" \
  -H "content-type: application/json" \
  -H "x-tandem-incident-monitor-intake-key: $INCIDENT_MONITOR_INTAKE_KEY" \
  -d '{
    "project_id": "external-service",
    "source_id": "ci",
    "report": {
      "title": "CI smoke failed",
      "detail": "The deployment smoke test failed after release.",
      "event": "ci.smoke.failed",
      "level": "error",
      "fingerprint": "ci-smoke-deploy-failure"
    }
  }'
```

## MCP Rules

Incident Monitor can publish through configured GitHub, Linear, webhook, telemetry, memory, or MCP destinations. That does not mean an agent should call those destinations directly.

Agents should:

- discover MCP tools through Tandem's MCP inventory when tool context matters
- treat missing MCP capability as a destination readiness problem
- use route preview to explain destination choice before publish
- leave destination mutation to Incident Monitor publish paths
- preserve approval gates for high-risk or external mutations
- read receipts after publish instead of assuming the external action succeeded

Agents should not:

- create GitHub or Linear issues directly when the user asked for governed Incident Monitor handling
- call arbitrary MCP tools to simulate publish
- use scoped intake keys to preview routes, inspect files, or publish
- send sensitive evidence to webhook or MCP destinations without redaction and approval

## External Sources

Use external sources when CI, a local service, or a long-running agent writes logs outside a Tandem workflow.

Important path rule for hosted installs:

| Path                           | Meaning                                                                 |
| ------------------------------ | ----------------------------------------------------------------------- |
| `/workspace/repos/<repo-name>` | Source checkout that Incident Monitor may inspect after Coder sync      |
| `/workspace/tandem-data`       | Runtime state, incidents, drafts, receipts, and config; not source code |

Configure external sources in `Settings -> Incident Monitor`, bind stable `project_id` and `source_id` values, and keep log paths inside the monitored `workspace_root`.

## Governance Evidence

Use these surfaces when an operator, auditor, or follow-on agent needs proof of what Tandem observed and enforced:

| Surface                                              | Purpose                                                                                                         |
| ---------------------------------------------------- | --------------------------------------------------------------------------------------------------------------- |
| `GET /incident-monitor/security/authority-inventory` | Read-only map of workflows, agents, MCP policy, destinations, sources, approvals, and external publish surfaces |
| `GET /incident-monitor/security/posture-checks`      | Deterministic governance findings over inventory and recent decisions                                           |
| `POST /incident-monitor/security/assessment-probes`  | Authorized dry-run checks for Tandem governance controls                                                        |
| `POST /incident-monitor/security/assessment-report`  | Redacted JSON and Markdown report with evidence refs and recommendations                                        |
| `POST /incident-monitor/security/deployment-cards`   | Production-governance cards for agents, workflows, sources, and Tandem self-monitoring                          |
| `GET`/`POST /incident-monitor/security/scenario-packs`   | Describe or run dry-run adversarial scenario packs against live routing, approval, and readiness logic          |
| `POST /incident-monitor/security/governance-metrics` | Redacted governance maturity metrics plus behavioral-drift detection                                            |
| `GET`/`POST /incident-monitor/security/reassessments`    | List or run versioned, fingerprint-deduplicated continuous reassessments                                       |
| `GET /incident-monitor/posts`                        | Destination-aware publish receipts and outcomes                                                                 |

Reports intentionally omit raw credentials, intake-key material, webhook secrets, auth headers, arbitrary destination config values, and raw protected-audit payloads by default.

For production readiness, use [Production Governance](../production-governance/) to map these evidence surfaces to deployment-card ownership, data readiness, destination policy, audit export, retention, incident response, and drift review decisions.

## Failure Handling

| Symptom                              | Agent response                                                                                   |
| ------------------------------------ | ------------------------------------------------------------------------------------------------ |
| Incident Monitor disabled or unready | Stop and ask for setup in `Settings -> Incident Monitor`; do not publish directly.               |
| Destination unready                  | Use route preview details and readiness errors in the incident or blocker note.                  |
| Missing MCP capability               | Treat it as an operator-visible capability gap; do not invent a parallel adapter.                |
| No route matches                     | Use default destination policy only if preview says it is effective and allowed.                 |
| Scoped key rejected                  | Check project ID, source ID, key status, and `incident_monitor:report` scope.                    |
| Approval denied                      | Keep the draft and evidence; do not retry publish through another surface.                       |
| Duplicate match found                | Add triage context or comment through the governed publish path instead of creating a new issue. |
| Retention/export policy missing      | Call it out before production use; reports and receipts need customer-owned evidence policy.     |

## Related

- [Incident Monitor Reference](../../reference/incident-monitor/)
- [Incident Monitor Setup Checklist](../setup-checklist/)
- [Incident Monitor Production Governance](../production-governance/)
- [Destination Router](../destination-router/)
- [External Sources](../external-sources/)
- [Destinations](../destinations/)
- [Security Posture Mode](../security-posture/)
- [Incident Monitor External Log Intake](../../incident-monitor-external-log-intake/)
- [Agent Runtime Contracts](../../agent-runtime-contracts/)
- [MCP Capability Discovery And Request Flow](../../mcp-capability-discovery-and-request-flow/)
- [TypeScript SDK](../../sdk/typescript/)
- [Python SDK](../../sdk/python/)
