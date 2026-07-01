---
title: Incident Monitor Reference
description: Use Tandem's Incident Monitor namespace to turn runtime failures and safety signals into governed drafts, approvals, receipts, and destination publishes.
---

Incident Monitor is Tandem's governed failure-intake pipeline.

Use it when a workflow failure, recurring runtime error, manual report, safety signal, or operator finding should become a reviewable draft instead of a direct external mutation.

## What it covers

- runtime failures from workflows, routines, sessions, and automations
- external project log intake from configured local log files
- scoped report intake from external systems without sharing the full engine token
- manual reports for operator-found issues or missing context
- triage runs that inspect, research, validate, and propose fixes
- draft approval and publishing when the backend is configured for it
- posts that represent already-published GitHub activity

## Typical flow

1. Check readiness with `getStatus()`.
2. Inspect incidents with `listIncidents()`.
3. Inspect drafts with `listDrafts()`.
4. Use triage helpers to create or refresh issue-ready drafts.
5. Approve, deny, or publish when the draft is ready.
6. Recheck the match or review the resulting posts.

Incident Monitor is intentionally not "report everything immediately to GitHub". It keeps intake, triage, and approval separate so the system can add evidence before anything leaves Tandem.

Incident Monitor is the destination-router evolution of this pipeline. GitHub, Linear, signed webhook, local telemetry, generic MCP tool, and internal memory destinations use the governed router model with source identity, routing, destination readiness, approval, and receipts. See [Incident Monitor Overview](../incident-monitor/overview/) and [Destination Router](../incident-monitor/destination-router/).

If you are an MCP-connected agent, start with [Incident Monitor Agent Runtime Guide](../incident-monitor/agent-runtime-guide/) before choosing a publish path.

## Control Panel Setup

Configure the destination-router surface from `Settings -> Incident Monitor`.

The setup panel is organized around:

- Sources: local directory, target repo, monitored external projects, log sources, and scoped intake keys.
- Destinations: GitHub-compatible legacy settings plus explicit GitHub, Linear, webhook, local telemetry, memory, and MCP destination rows.
- Routing: default destinations, ordered routes, match rules, route tags, source bindings, and route preview.
- Safety Defaults: approval, redaction, unready destination blocking, and retention defaults.

Legacy GitHub setup remains compatible: if no explicit router destination is configured, Tandem still treats the existing GitHub posting settings as the default Incident Monitor destination. New destinations and routes are admin/full-token config mutations; scoped intake keys are report-only and cannot change routes, preview destination details, call tools, or publish issues.

## External Project Log Intake

Incident Monitor can also watch local logs for projects outside a Tandem workflow. Configure `monitored_projects` in `Settings -> Incident Monitor`, then use the external-project panel to inspect source health, create scoped intake keys, reset offsets, and replay the latest log candidate.

Use this path when CI, ACA, or another local service writes failures to JSON-lines or plaintext logs and should produce governed Incident Monitor incidents.

On hosted installs, Coder and Incident Monitor share repositories under `/workspace/repos`. Sync the repo from the Coder page first, then set Incident Monitor's local directory to `/workspace/repos/<repo-name>` so triage can inspect the source tree. `/workspace/tandem-data` is runtime state, not source code.

For setup steps, examples, and agent-facing guidance, see [Incident Monitor External Log Intake](../incident-monitor-external-log-intake/).

## Signed Webhook Destinations

Use a `webhook` destination when an incident should be delivered to a customer-owned HTTP endpoint instead of GitHub or Linear.

Webhook destinations require:

- `webhook_url`: an HTTPS URL by default.
- `webhook_secret_ref`: an env-backed reference such as `env:TANDEM_WEBHOOK_SECRET`.
- Optional `config.allowed_hosts` when the destination should be restricted to specific hostnames.

Tandem signs each delivery with `x-tandem-signature: t=<timestamp_ms>,v1=<hmac_sha256>` over `<timestamp_ms>.<raw_json_body>` and includes `x-tandem-signature-scheme: tandem_hmac_sha256_v1`, `x-tandem-delivery-id`, `x-tandem-event`, and `idempotency-key` headers.

Private, localhost, link-local, and otherwise internal URL ranges are blocked by default. Development and test-only destinations can opt into `allow_private_networks` and `allow_insecure_http`, but production webhook destinations should use HTTPS, a public endpoint, and a host allowlist. Payloads and response excerpts are bounded, retry attempts are capped, and every delivery records a destination-specific Incident Monitor post receipt with status, attempt count, status code, delivery ID, route metadata, and evidence digest.

## Authority Inventory

Use `GET /incident-monitor/security/authority-inventory` to collect a read-only authority map for Incident Monitor security posture assessment.

The response includes:

- workflows, workflow hooks, Automation V2 specs, agents, tool policies, and MCP policy
- MCP server/tool inventory summaries
- Incident Monitor destinations, routes, monitored sources, scoped intake keys, and default destination policy
- approval rules, pending approvals, governance approval requests, and recent policy decisions
- external publish surfaces and recent external-action identifiers
- tenant/workspace context where Tandem has it

The inventory is designed for audit evidence and future posture findings. It returns identifiers and field-presence signals, but omits raw intake keys, key hashes, webhook secret values, auth headers, destination config values, action receipts, and arbitrary metadata values. Scoped intake keys cannot access this endpoint.

## Security Assessment Reports

Use `POST /incident-monitor/security/assessment-report` to generate a redacted Incident Monitor security gap assessment report. The endpoint requires the full admin token, runs read-only posture checks and optional dry-run controlled probes, summarizes incidents and destination receipts, and persists a context-run evidence artifact by default.

The report includes JSON sections plus a Markdown summary:

- assessment scope, monitored sources, authority inventory, destinations, routes, and approval coverage
- posture findings, controlled probe results, evidence refs, recommendations, residual risk, and follow-up actions
- Tandem self-monitoring boundaries using `source_kind=tandem_runtime` and `source_kind=tandem_monitor`
- protected audit export summaries for Incident Monitor route decisions, publish attempts, monitor events, and control failures
- a non-mutating destination route preview for report export

Reports do not embed raw protected-audit payloads, scoped intake keys, auth headers, webhook secret values, or destination receipt payloads by default. High-assurance deployments should export protected audit evidence to a customer-owned system of record such as SIEM, database, object storage, Linear, GitHub, webhook, telemetry, or an approved MCP destination. Tandem can show what it observed and enforced; it does not independently prove itself safe.

## TypeScript

```typescript
import { TandemClient } from "@frumu/tandem-client";

const client = new TandemClient({
  baseUrl: "http://localhost:39731",
  token: process.env.TANDEM_ENGINE_TOKEN!,
});

const status = await client.incidentMonitor.getStatus();
if (status.status?.readiness?.enabled === false) {
  console.log("Incident Monitor is disabled or missing config");
}

const incidents = await client.incidentMonitor.listIncidents({ limit: 20 });
const drafts = await client.incidentMonitor.listDrafts({ limit: 20 });
const destinations = await client.incidentMonitor.listDestinations();
const authority = await client.incidentMonitor.getAuthorityInventory();
const report = await client.incidentMonitor.generateAssessmentReport({
  source_kind: "tandem_monitor",
  routeDestinationIds: destinations.map((destination) => destination.destination_id),
});

if (drafts.drafts[0]) {
  await client.incidentMonitor.createTriageRun(drafts.drafts[0].draft_id);
  await client.incidentMonitor.publishDraftToDestinations(
    drafts.drafts[0].draft_id,
    destinations.map((destination) => destination.destination_id)
  );
}

await client.incidentMonitor.report({
  title: "Workflow failed while establishing GitHub context",
  detail: "The automation timed out before triage could complete.",
  source: "automation_v2",
  event: "automation_v2.run.failed",
  level: "error",
});
```

## Python

```python
from tandem_client import TandemClient

async with TandemClient(base_url="http://localhost:39731", token="...") as client:
    status = await client.incident_monitor.get_status()
    incidents = await client.incident_monitor.list_incidents(limit=20)
    drafts = await client.incident_monitor.list_drafts(limit=20)
    destinations = await client.incident_monitor.list_destinations()

    if drafts.drafts:
        await client.incident_monitor.create_triage_run(drafts.drafts[0].draft_id)
        await client.incident_monitor.publish_draft_to_destinations(
            drafts.drafts[0].draft_id,
            [destination.destination_id for destination in destinations],
        )

    authority = await client.incident_monitor.get_authority_inventory()
    report = await client.incident_monitor.generate_assessment_report(
        source_kind="tandem_monitor",
        route_destination_ids=[destination.destination_id for destination in destinations],
    )
    cards = await client.incident_monitor.generate_deployment_cards(
        defaults={
            "business_owner": "Security Ops",
            "accountable_team": "AI Governance",
            "autonomy_level": "supervised",
            "data_classification": "internal",
            "approval_protocol": "Human approval before high-risk publish",
            "escalation_protocol": "Escalate regulatory/legal risk to security lead",
            "review_cadence_days": 30,
        },
        metadata={
            "tandem:self_monitoring:incident_monitor": {
                "intended_purpose": "Route governed incident evidence",
            }
        },
    )

    await client.incident_monitor.report({
        "title": "Workflow failed while establishing GitHub context",
        "detail": "The automation timed out before triage could complete.",
        "source": "automation_v2",
        "event": "automation_v2.run.failed",
        "level": "error",
    })
```

## Useful methods

- `getStatus()` / `get_status()`
- `getAuthorityInventory()` / `get_authority_inventory()`
- `generateAssessmentReport()` / `generate_assessment_report()`
- `generateDeploymentCards()` / `generate_deployment_cards()`
- `recomputeStatus()` / `recompute_status()`
- `pause()` / `pause()`
- `resume()` / `resume()`
- `debug()` / `debug()`
- `listIncidents()` / `list_incidents()`
- `getIncident()` / `get_incident()`
- `replayIncident()` / `replay_incident()`
- `listDrafts()` / `list_drafts()`
- `getDraft()` / `get_draft()`
- `createTriageRun()` / `create_triage_run()`
- `createTriageSummary()` / `create_triage_summary()`
- `approveDraft()` / `approve_draft()`
- `denyDraft()` / `deny_draft()`
- `createIssueDraft()` / `create_issue_draft()`
- `publishDraft()` / `publish_draft()`
- `recheckMatch()` / `recheck_match()`
- `listPosts({ destinationId })` / `list_posts(destination_id=...)`
- `previewRoute()` / `preview_route()`
- `listDestinations()` / `list_destinations()`
- `upsertDestination()` / `upsert_destination()`
- `removeDestination()` / `remove_destination()`
- `listRoutes()` / `list_routes()`
- `upsertRoute()` / `upsert_route()`
- `removeRoute()` / `remove_route()`
- `publishDraftToDestinations()` / `publish_draft_to_destinations()`
- `listIntakeKeys()`
- `createIntakeKey()`
- `disableIntakeKey()`
- `resetLogSourceOffset()`
- `replayLatestLogSourceCandidate()`

## Safety notes

- A report creates intake, not an automatic GitHub mutation.
- Drafts remain reviewable until approval or publish is explicitly requested.
- Scoped intake keys can report only for their configured project/scope and cannot use config, route-preview, normal report, publish, or intake-key management routes.
- Destination and route mutations require the full engine API token.
- Destination/route config changes, intake-key lifecycle changes, and destination-router publish attempts/outcomes emit redacted audit events and protected audit-ledger rows.
- Authority inventory is read-only and returns summarized evidence; it must not expose raw credentials, intake-key material, action receipts, or secret-backed destination values.
- Deployment cards are read-only production-governance artifacts generated from authority inventory plus operator metadata. Missing required owner, accountability, escalation, data classification, or review fields return posture findings instead of silently passing.
- Webhook destinations should use HTTPS, host allowlists, and env-backed secrets.
- Secret redaction is enabled by default for Incident Monitor safety defaults. Report-level `redaction_profile` and source bindings can add stricter profiles for specific projects or sources.
- `retention_days` is unset by default, so deployments should configure retention/export policy for reports, receipts, and protected audit evidence before production use. Source bindings can attach `retention_profile` labels for downstream policy.
- Reset/replay log-source actions require the full engine API token.
- Status can be blocked by missing config, missing repo access, or missing runtime capabilities.
- Missing fields should be handled defensively; Incident Monitor records are intentionally flexible.

## Related

- [SDK Overview](../sdk/)
- [TypeScript SDK](../sdk/typescript/)
- [Python SDK](../sdk/python/)
- [Control Panel](../control-panel/)
- [Incident Monitor Overview](../incident-monitor/overview/)
- [Incident Monitor Agent Runtime Guide](../incident-monitor/agent-runtime-guide/)
