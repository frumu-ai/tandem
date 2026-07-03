# Incident Monitor — Continuous Reassessment & Change-Triggered Reviews

Governance reassessment is continuous. The Incident Monitor re-runs its
authority / data-readiness / routing / approval / destination / incident
posture on a **cadence** and on selected **configuration or runtime change
events**, producing a **versioned result** that compares the current findings
against the previous run. Everything runs read-only and **never mutates external
systems** — a finding is escalated through the normal Incident Monitor draft /
approval / routing path, not by the scheduler.

## What a reassessment produces

Each run writes a versioned `ReassessmentRecord` for a scope (currently the
tenant `deployment` scope):

- `version` — increments per scope, so history is ordered.
- `trigger` — `scheduled`, `manual`, or the change event that scheduled it.
- `findings` — normalized, **redacted** posture findings (rule id, scope,
  severity, category, evidence refs, and a stable `fingerprint`). Free-text
  finding messages and payloads are never carried.
- `comparison` — `new` / `recurring` / `resolved` fingerprints versus the
  previous run. Recurring findings keep their `first_seen_at_ms` and increment
  `occurrence_count` instead of adding duplicate noise.
- `mode: "dry_run"`, `mutates_external_systems: false`.

Findings are sourced by re-running the Phase-8 assessment surfaces (posture
findings plus governance-maturity metric breaches / behavioral drift).

## Triggers

The scheduler runs a scope when it is **due by cadence** or when a **change
event** has scheduled it. Change events are detected by diffing the
governance-relevant sections of the Incident Monitor config when it is saved:

| Change | Trigger |
| --- | --- |
| Destination / route / default-destination edit | `destination_or_route_change` |
| Monitored source / schema / freshness / lineage edit | `monitored_source_change` |
| Tenant/workspace binding change | `tenant_boundary_change` |
| Model/provider policy change | `model_policy_change` |
| MCP server change | `mcp_inventory_change` |
| Approval/escalation policy change | `approval_policy_change` |

Subsystems outside the Incident Monitor config (workflow/agent authority, MCP
tool inventory, recurring-incident thresholds, compliance-mapping versions) call
`note_incident_monitor_reassessment_trigger(...)` to schedule a reassessment
with the corresponding trigger. Change triggers can be disabled per config
(`reassessment.change_triggers_enabled`).

## Schedule status

For every scope a tenant has assessed (and the deployment scope even before its
first run) the system derives:

- `last_completed_at_ms` — the newest run's timestamp (absent if never run).
- `next_due_at_ms` — `last_completed + cadence` (or now if never run).
- `overdue` — past due by more than the grace period.

This status is surfaced on the reassessments endpoint and folded into the
deployment-cards payload under `reassessment.schedule`, so operators can see next
due / last completed / overdue per deployment card, source, and workflow.

## Configuration

Operator-tunable, with production-safe defaults, under
`config.incident_monitor.reassessment`:

```json
{
  "cadence_ms": 604800000,
  "overdue_grace_ms": 86400000,
  "change_triggers_enabled": true
}
```

- `cadence_ms` — reassessment cadence (default 7 days).
- `overdue_grace_ms` — grace past the due date before a scope is flagged overdue
  (default 1 day).
- `change_triggers_enabled` — whether config-change events schedule an immediate
  reassessment.

## Endpoint

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/incident-monitor/security/reassessments` | Run a reassessment now (dry-run, redacted) and return the versioned record + schedule. |
| `GET` | `/incident-monitor/security/reassessments` | List recent records and per-scope schedule status. |

Admin-only (full API token; scoped intake keys rejected). A background scheduler
also runs due and change-triggered scopes automatically. Each run appends a
protected audit event (`incident_monitor.reassessment.completed`); each scheduled
change appends `incident_monitor.reassessment.triggered`.
