---
title: Bug Monitor And Issue Reporter
description: Use Tandem's Bug Monitor namespace to turn runtime failures into governed drafts, approvals, and published issues.
---

Bug Monitor is Tandem's governed failure-intake pipeline.

Use it when a workflow failure, recurring runtime error, manual report, or operator finding should become a reviewable draft instead of a direct external mutation.

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

Bug Monitor is intentionally not "report everything immediately to GitHub". It keeps intake, triage, and approval separate so the system can add evidence before anything leaves Tandem.

Incident Monitor is the destination-router evolution of this pipeline. GitHub, Linear, and signed webhook destinations use the governed router today, and the same source identity, routing, destination readiness, approval, and receipt model is available for future telemetry/database, MCP tool, and internal memory destinations. See [Incident Monitor Overview](../incident-monitor/overview/) and [Destination Router](../incident-monitor/destination-router/).

## External Project Log Intake

Bug Monitor can also watch local logs for projects outside a Tandem workflow. Configure `monitored_projects` in `Settings -> Bug Monitor`, then use the external-project panel to inspect source health, create scoped intake keys, reset offsets, and replay the latest log candidate.

Use this path when CI, ACA, or another local service writes failures to JSON-lines or plaintext logs and should produce governed Bug Monitor incidents.

On hosted installs, Coder and Bug Monitor share repositories under `/workspace/repos`. Sync the repo from the Coder page first, then set Bug Monitor's local directory to `/workspace/repos/<repo-name>` so triage can inspect the source tree. `/workspace/tandem-data` is runtime state, not source code.

For setup steps, examples, and agent-facing guidance, see [Bug Monitor External Log Intake](../bug-monitor-external-log-intake/).

## Signed Webhook Destinations

Use a `webhook` destination when an incident should be delivered to a customer-owned HTTP endpoint instead of GitHub or Linear.

Webhook destinations require:

- `webhook_url`: an HTTPS URL by default.
- `webhook_secret_ref`: an env-backed reference such as `env:TANDEM_WEBHOOK_SECRET`.
- Optional `config.allowed_hosts` when the destination should be restricted to specific hostnames.

Tandem signs each delivery with `x-tandem-signature: t=<timestamp_ms>,v1=<hmac_sha256>` over `<timestamp_ms>.<raw_json_body>` and includes `x-tandem-signature-scheme: tandem_hmac_sha256_v1`, `x-tandem-delivery-id`, `x-tandem-event`, and `idempotency-key` headers.

Private, localhost, link-local, and otherwise internal URL ranges are blocked by default. Development and test-only destinations can opt into `allow_private_networks` and `allow_insecure_http`, but production webhook destinations should use HTTPS, a public endpoint, and a host allowlist. Payloads and response excerpts are bounded, retry attempts are capped, and every delivery records a destination-specific Bug Monitor post receipt with status, attempt count, status code, delivery ID, route metadata, and evidence digest.

## TypeScript

```typescript
import { TandemClient } from "@frumu/tandem-client";

const client = new TandemClient({
  baseUrl: "http://localhost:39731",
  token: process.env.TANDEM_ENGINE_TOKEN!,
});

const status = await client.bugMonitor.getStatus();
if (status.status?.readiness?.enabled === false) {
  console.log("Bug Monitor is disabled or missing config");
}

const incidents = await client.bugMonitor.listIncidents({ limit: 20 });
const drafts = await client.bugMonitor.listDrafts({ limit: 20 });

if (drafts.drafts[0]) {
  await client.bugMonitor.createTriageRun(drafts.drafts[0].draft_id);
}

await client.bugMonitor.report({
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
    status = await client.bug_monitor.get_status()
    incidents = await client.bug_monitor.list_incidents(limit=20)
    drafts = await client.bug_monitor.list_drafts(limit=20)

    if drafts.drafts:
        await client.bug_monitor.create_triage_run(drafts.drafts[0].draft_id)

    await client.bug_monitor.report({
        "title": "Workflow failed while establishing GitHub context",
        "detail": "The automation timed out before triage could complete.",
        "source": "automation_v2",
        "event": "automation_v2.run.failed",
        "level": "error",
    })
```

## Useful methods

- `getStatus()` / `get_status()`
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
- `listPosts()` / `list_posts()`
- `listIntakeKeys()`
- `createIntakeKey()`
- `disableIntakeKey()`
- `resetLogSourceOffset()`
- `replayLatestLogSourceCandidate()`

## Safety notes

- A report creates intake, not an automatic GitHub mutation.
- Drafts remain reviewable until approval or publish is explicitly requested.
- Scoped intake keys can report only for their configured project/scope.
- Reset/replay log-source actions require the full engine API token.
- Status can be blocked by missing config, missing repo access, or missing runtime capabilities.
- Missing fields should be handled defensively; Bug Monitor records are intentionally flexible.

## Related

- [SDK Overview](../sdk/)
- [TypeScript SDK](../sdk/typescript/)
- [Python SDK](../sdk/python/)
- [Control Panel](../control-panel/)
- [Incident Monitor Overview](../incident-monitor/overview/)
