---
title: Bug Monitor And Issue Reporter
description: Use Tandem's Bug Monitor namespace to turn runtime failures into governed drafts, approvals, and published issues.
---

Bug Monitor is Tandem's governed failure-intake pipeline.

Use it when a workflow failure, recurring runtime error, manual report, or operator finding should become a reviewable draft instead of a direct external mutation.

## What it covers

- runtime failures from workflows, routines, sessions, and automations
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

## Safety notes

- A report creates intake, not an automatic GitHub mutation.
- Drafts remain reviewable until approval or publish is explicitly requested.
- Status can be blocked by missing config, missing repo access, or missing runtime capabilities.
- Missing fields should be handled defensively; Bug Monitor records are intentionally flexible.

## Related

- [SDK Overview](../sdk/)
- [TypeScript SDK](../sdk/typescript/)
- [Python SDK](../sdk/python/)
- [Control Panel](../control-panel/)
