# Incident Monitor — Governance Maturity Metrics & Behavioral Drift

Governance maturity metrics turn the Incident Monitor's audit events, incidents,
publish receipts, and policy decisions into actionable production-health signals,
and compare the current window against a baseline window to detect **behavioral
drift**. Everything runs read-only and **never mutates external systems**; output
is **redacted** (counts, rates, ids, and hashes only) and threshold breaches are
surfaced as dry-run posture findings.

## Metrics

Each metric reports `numerator`, `denominator`, `rate`, its `threshold`,
`breached`, `evaluable`, `missing_evidence_reasons`, and `evidence_refs`.

| Metric | Definition |
| --- | --- |
| `governance_confidence` | Share of high-risk incidents with a complete audit trail (evidence refs **and** an auditable publish trail — a receipt or a `incident_monitor.publish.*` protected-audit event). |
| `authority_boundary_compliance` | Share of allow/deny policy decisions that were **allowed** (stayed within configured authority). |
| `escalation_pathway_utilization` | Share of escalation-eligible (`approval_required`) policy decisions that reached human review (carry an `approval_id`). |
| `route_readiness_compliance` | Share of enabled sources and destinations that are ready. |
| `receipt_completeness` | Share of completed publish receipts that are `posted` with an external reference. |
| `recurring_incident_rate` | Share of incidents observed more than once (signal only, no threshold). |

A metric whose window has no data (`denominator == 0`) is reported as
`evaluable: false` with its `missing_evidence_reasons`, not a breach.

## Behavioral drift

Drift compares the **current** window `(to_ms - window_ms, to_ms]` against the
**baseline** window immediately before it, flagging any signal whose rate moved
by more than `drift_rate_delta_max`. Signals: `publish_failure_rate`,
`denied_action_rate`, `escalation_rate`, and per-category
`incident_category_share:<category>`.

## Thresholds

Production-safe defaults ship built in and can be overridden per request:

```json
{
  "governance_confidence_min": 0.9,
  "authority_boundary_compliance_min": 0.95,
  "escalation_utilization_min": 0.9,
  "route_readiness_min": 0.9,
  "receipt_completeness_min": 0.95,
  "drift_rate_delta_max": 0.25
}
```

A metric below its minimum, or a drift delta above `drift_rate_delta_max`,
produces a dry-run `threshold_findings` entry (metric breaches carry an Incident
Monitor draft suggestion so an operator can escalate them through the normal
workflow).

## Endpoint

| Method | Path | Purpose |
| --- | --- | --- |
| `POST` | `/incident-monitor/security/governance-metrics` | Compute metrics + drift in dry-run, redacted mode. |

Admin-only (full API token; scoped intake keys rejected). Optional body:

```json
{ "to_ms": 0, "window_ms": 604800000, "thresholds": { "...": 0.0 } }
```

- `to_ms` — end of the current window (defaults to now).
- `window_ms` — window duration (defaults to 7 days).
- `thresholds` — optional overrides.

The same metrics + drift are folded into the Phase-8 security assessment report
(`/incident-monitor/security/assessment-report`) under
`sections.governance_maturity_metrics`, and persisted in its evidence pack.
