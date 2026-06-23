# Bug Monitor External Log Intake Demo

This fixture exercises the external-project intake path without requiring a workflow run.

## Fixture

- Example log: `docs/fixtures/bug-monitor-external-log-intake/service.log.jsonl`
- Source format: JSON-lines
- Expected result: one Bug Monitor incident/draft for `external_service_crash`

## Example Config

Use a workspace-local copy of this repository when testing path validation.

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
        "source_kind": "external_app",
        "allowed_destination_ids": ["legacy-github"],
        "default_destination_ids": ["legacy-github"],
        "default_route_tags": ["external-demo"],
        "tenant_id": "local",
        "workspace_id": "demo",
        "approval_policy": "inherit",
        "log_sources": [
          {
            "source_id": "service-jsonl",
            "path": "docs/fixtures/bug-monitor-external-log-intake/service.log.jsonl",
            "source_kind": "ci",
            "format": "json",
            "minimum_level": "error",
            "start_position": "beginning",
            "watch_interval_seconds": 5,
            "default_route_tags": ["service-jsonl"],
            "default_destination_ids": ["legacy-github"],
            "approval_policy": "inherit"
          }
        ]
      }
    ]
  }
}
```

## Source Binding

`monitored_projects` is the configured source boundary for systems outside Tandem. Tandem treats the saved project/source config as authoritative for:

- source identity: `project_id`, `source_id`, and `source_kind`
- inspection boundary: `workspace_root` plus log paths that must stay inside it
- routing context: `default_route_tags`, `default_destination_ids`, and `allowed_destination_ids`
- tenancy context: `tenant_id`, `workspace_id`, and `event_schema_version`
- safety defaults: `approval_policy`, `redaction_profile`, and `retention_profile`

Log watcher submissions and scoped intake reports inherit these configured values. Scoped intake keys can submit events for their project, but they cannot override source identity, allowed destinations, route tags, tenant/workspace context, or approval policy.

Route preview can use `project_id` and `log_source_id` to show the route that would match a sample event. If a route selects a destination outside the source `allowed_destination_ids`, preview marks it blocked and publishing fails closed.

## Smoke Path

1. Save the config through Settings -> Incident Monitor.
2. Confirm the external project panel shows one enabled project, one enabled source, route tags, and allowed/default destinations.
3. Wait for the watcher to poll.
4. Confirm the source health reports a candidate/submission count.
5. Confirm Bug Monitor incidents include the fixture failure with a `tandem://bug-monitor/...` evidence ref.

For live testing, append a new JSON line with a distinct `fingerprint` or error message so dedupe cooldown does not suppress the candidate.

## Smoke Script

After saving the example config, this script appends a unique fixture error, resets the demo source offset, and polls Bug Monitor incidents until the matching fingerprint appears:

```bash
TANDEM_BASE_URL=http://localhost:3000/api/engine \
TANDEM_TOKEN="$TANDEM_TOKEN" \
node scripts/bug-monitor-external-log-intake-smoke.mjs
```

Set `BUG_MONITOR_DEMO_PROJECT_ID` or `BUG_MONITOR_DEMO_SOURCE_ID` if your saved config uses different ids.
