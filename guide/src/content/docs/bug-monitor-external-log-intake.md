---
title: Bug Monitor External Log Intake
description: Configure Tandem Bug Monitor to watch external project logs, accept scoped reports, and help agents diagnose failures without a workflow run.
---

Bug Monitor external log intake lets Tandem watch another local project for failures without requiring that project to run a Tandem workflow.

Use this when an external app, CI job, or long-running agent writes logs that should become Bug Monitor incidents and issue drafts.

## What Agents Should Know

- Bug Monitor can watch configured local log files under a monitored project's `workspace_root`.
- The watcher accepts JSON-lines logs and plaintext stack traces.
- Watched log files must stay inside the configured workspace root; path escapes and symlink escapes are rejected.
- External reporters can submit failures with scoped intake keys instead of the full engine token.
- Scoped intake keys can only report for their configured project and scope. They cannot change config, run workflows, publish issues, call tools, or read files.
- Reset/replay debug actions require the full engine API token because they mutate watcher state.
- GitHub posting is still governed by Bug Monitor draft, approval, and publish policy.

## Setup Checklist

1. Open `Settings -> Bug Monitor`.
2. Enable Bug Monitor and set the target GitHub repo.
3. Select the MCP server Bug Monitor should use for GitHub issue lookup/publish.
4. Add or generate `monitored_projects` JSON.
5. Save the Bug Monitor config.
6. Confirm the external-project panel shows the project and source health.
7. Create a scoped intake key if CI or another external service needs to report directly.
8. Keep `auto_create_new_issues` and `require_approval_for_new_issues` aligned with your team's policy.

## Hosted Workspace Layout

Hosted Tandem installs share the same checked-out repositories between Coder and Bug Monitor.

Use Coder's `Sync repo` action first, then set Bug Monitor's local directory to the synced repo folder:

```text
/workspace/repos/<repo-name>
```

For example, the `frumu-ai/tandem` repo should use:

```text
/workspace/repos/tandem
```

Other hosted paths are for runtime state:

| Path                           | Purpose                                                                         |
| ------------------------------ | ------------------------------------------------------------------------------- |
| `/workspace/repos`             | Shared source checkouts created by Coder sync.                                  |
| `/workspace/repos/<repo-name>` | The repo folder Bug Monitor should inspect.                                     |
| `/workspace/aca/repos`         | Compatibility mount for ACA/Coder internals; it points at the same repo volume. |
| `/workspace/tandem-data`       | Runtime config, incidents, drafts, logs, and state; not the source checkout.    |

## Monitored Project Config

Minimal example:

```json
{
  "bug_monitor": {
    "enabled": true,
    "repo": "frumu-ai/tandem",
    "monitored_projects": [
      {
        "project_id": "external-demo",
        "name": "External demo service",
        "enabled": true,
        "repo": "frumu-ai/tandem",
        "workspace_root": "/workspace/tandem",
        "log_sources": [
          {
            "source_id": "service-jsonl",
            "path": "docs/fixtures/bug-monitor-external-log-intake/service.log.jsonl",
            "format": "json",
            "minimum_level": "error",
            "start_position": "beginning",
            "watch_interval_seconds": 5
          }
        ]
      }
    ]
  }
}
```

Important fields:

| Field                     | Meaning                                                                    |
| ------------------------- | -------------------------------------------------------------------------- |
| `project_id`              | Stable id used by status, intake keys, and debug actions.                  |
| `repo`                    | GitHub repo slug for incidents and drafts.                                 |
| `workspace_root`          | Local root Tandem may inspect for this external project.                   |
| `log_sources[].source_id` | Stable id for one watched file.                                            |
| `log_sources[].path`      | Relative path under `workspace_root`, or an absolute path still inside it. |
| `format`                  | `auto`, `json`, or `plaintext`.                                            |
| `minimum_level`           | Usually `error`; use `warn` only if you are ready for more noise.          |
| `start_position`          | `end` for production, `beginning` for fixtures and replay demos.           |

## JSON Log Shape

JSON-lines logs should include as many of these fields as possible:

```json
{
  "timestamp": "2026-05-03T01:01:00Z",
  "level": "error",
  "service": "external-demo",
  "event": "external_service_crash",
  "message": "worker failed while processing GitHub issue sync",
  "error": "TypeError: Cannot read properties of undefined",
  "stack": "TypeError: ...\n    at syncIssueWorkflow (/workspace/src/sync.ts:42:17)",
  "fingerprint": "external-demo-issue-sync-workflow-id"
}
```

If no `fingerprint` is provided, Tandem computes one from the failure content. Providing a stable fingerprint helps dedupe repeat failures.

## Scoped Intake Keys

Create keys from `Settings -> Bug Monitor -> Scoped intake keys` or the TypeScript SDK.

```typescript
const created = await client.bugMonitor.createIntakeKey({
  project_id: "external-demo",
  name: "CI reporter",
  scopes: ["bug_monitor:report"],
});

console.log(created.raw_key); // Store once. Tandem only persists the hash.
```

Use the raw key to report from an external process:

```bash
curl -X POST "$TANDEM_BASE_URL/bug-monitor/intake/report" \
  -H "content-type: application/json" \
  -H "x-tandem-bug-monitor-intake-key: $BUG_MONITOR_INTAKE_KEY" \
  -d '{
    "project_id": "external-demo",
    "source_id": "ci",
    "report": {
      "title": "CI smoke failed",
      "detail": "The fixture smoke failed after deploy.",
      "event": "ci.smoke.failed",
      "level": "error",
      "fingerprint": "ci-smoke-deploy-failure"
    }
  }'
```

The key is scoped to the project id and scope. If a reporter sends a different project id, Tandem rejects it.

## Debug Actions

Use these when helping a human test or recover a watched source.

```typescript
await client.bugMonitor.resetLogSourceOffset("external-demo", "service-jsonl");

const replay = await client.bugMonitor.replayLatestLogSourceCandidate(
  "external-demo",
  "service-jsonl"
);

console.log(replay.incident.incident_id, replay.draft?.draft_id);
```

Resetting the offset:

- sets the source offset back to byte `0`
- clears partial-line state
- clears recent fingerprint cooldowns
- updates the source runtime status

Replaying the latest candidate:

- reuses the latest log-backed incident for that project/source
- requires stored `offset_start` and `offset_end` evidence
- fails closed if the log file changed and the offsets no longer parse

## Smoke Fixture

The repository includes a demo log:

```text
docs/fixtures/bug-monitor-external-log-intake/service.log.jsonl
```

Dry-run the smoke script in CI-safe mode:

```bash
npm run bug-monitor:fixture:test
node scripts/bug-monitor-external-log-intake-smoke.mjs --dry-run
```

Run a live local smoke after saving the example monitored project config:

```bash
TANDEM_BASE_URL=http://localhost:3000/api/engine \
TANDEM_TOKEN="$TANDEM_TOKEN" \
node scripts/bug-monitor-external-log-intake-smoke.mjs
```

The live smoke appends a unique JSONL error, resets the configured source offset, and polls Bug Monitor incidents until the matching fingerprint appears.

## Teaching Humans

When explaining this feature to an operator:

1. Start with the safety model: watched logs and scoped report keys create Bug Monitor intake, not direct GitHub mutations.
2. Ask where the external project lives on disk and which log file contains actionable failures.
3. Use the starter generator in Settings to create the first `monitored_projects` block.
4. Save config and watch source health before creating any intake keys.
5. Create scoped keys only for reporters that cannot use the full engine token.
6. Use reset offset for fixture/demo validation, not casually on noisy production logs.
7. Use replay latest only when the operator wants to deliberately reprocess the most recent candidate.

## Troubleshooting

| Symptom                | Likely Cause                                                | Fix                                                           |
| ---------------------- | ----------------------------------------------------------- | ------------------------------------------------------------- |
| Source stays `Waiting` | The watcher has not polled or the path is empty.            | Check `watch_interval_seconds`, file path, and server logs.   |
| Source is `Unhealthy`  | Path validation, file read, or parse error.                 | Inspect `last_error` in Settings.                             |
| Intake key rejected    | Wrong project id, disabled key, or missing scope.           | List keys and confirm `project_id` plus `bug_monitor:report`. |
| Replay returns 404     | No log-backed incident exists yet.                          | Let the watcher ingest a candidate first.                     |
| Replay returns 400     | Stored offsets no longer match the current file.            | Reset offset and let the watcher ingest a fresh candidate.    |
| Too many drafts        | Fingerprints are too noisy or `minimum_level` is too broad. | Add stable fingerprints and use `minimum_level: "error"`.     |

## Related

- [Bug Monitor And Issue Reporter](./reference/bug-monitor/)
- [Control Panel](./control-panel/)
- [Engine Authentication For Agents](./engine-authentication-for-agents/)
- [TypeScript SDK](./sdk/typescript/)
