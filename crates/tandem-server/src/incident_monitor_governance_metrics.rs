//! Governance maturity metrics and behavioral drift monitoring (TAN-488).
//!
//! Aggregates the Incident Monitor's audit events, incidents, publish receipts,
//! and policy decisions into production maturity metrics — governance confidence,
//! authority-boundary compliance, escalation pathway utilization, plus route
//! readiness / receipt completeness / recurring incidents — and compares the
//! current window against a baseline window to flag behavioral drift. Every
//! metric carries numerator/denominator, missing-evidence reasons, and evidence
//! refs. Output is redacted (counts, rates, ids, and hashes only) and threshold
//! breaches are surfaced as dry-run posture findings.

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Value};
use tandem_types::TenantContext;

use crate::{
    AppState, IncidentMonitorGovernanceThresholds, IncidentMonitorIncidentRecord,
    IncidentMonitorPostRecord,
};

const DEFAULT_WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1_000;

/// Compute the governance maturity metrics, behavioral drift, and threshold
/// breaches for a time window. `to_ms` is the (exclusive-ish) end of the current
/// window; the current window is `(to_ms - window_ms, to_ms]` and the baseline
/// window is the `window_ms` span immediately before it.
pub async fn compute_incident_monitor_governance_metrics(
    state: &AppState,
    tenant_context: &TenantContext,
    to_ms: u64,
    window_ms: Option<u64>,
    thresholds: &IncidentMonitorGovernanceThresholds,
) -> Value {
    let window_ms = window_ms
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_WINDOW_MS);
    let current = TimeWindow::current(to_ms, window_ms);
    let baseline = TimeWindow::baseline(to_ms, window_ms);

    // Tenant filter is applied inside the listing, before the recency cap, so a
    // scoped view can't lose the caller tenant's incidents to newer incidents
    // from other tenants (TAN-546-style crowd-out).
    let all_incidents = state
        .list_incident_monitor_incidents_for_tenant(tenant_context, 200)
        .await;
    let all_posts = state
        .list_incident_monitor_posts_for_tenant(tenant_context, 200)
        .await;
    let all_decisions = state.list_policy_decisions(tenant_context, 500).await;
    let audit_events =
        crate::audit::load_protected_audit_events_for_tenant(state, tenant_context).await;
    let status = state.incident_monitor_status_snapshot().await;

    // Scope route readiness to the tenant's own topology (TAN-547): the status
    // snapshot is operator-global, so for a tenant-scoped call keep only the
    // destinations/sources visible to that tenant. Local/implicit callers see
    // everything.
    let route_scope = tenant_visible_route_scope(state, tenant_context).await;

    // Incident ids that produced an auditable publish trail (a receipt or a
    // protected audit publish event referencing the incident).
    let mut audited_incident_ids: HashSet<String> = all_posts
        .iter()
        .filter_map(|post| post.incident_id.clone())
        .collect();
    for event in &audit_events {
        if event.event_type.starts_with("incident_monitor.publish.") {
            if let Some(incident_id) = event
                .payload
                .get("incident_id")
                .and_then(Value::as_str)
                .filter(|value| !value.trim().is_empty())
            {
                audited_incident_ids.insert(incident_id.to_string());
            }
        }
    }

    let metrics = vec![
        governance_confidence_metric(&all_incidents, &audited_incident_ids, &current, thresholds),
        authority_boundary_compliance_metric(&all_decisions, &current, thresholds),
        escalation_pathway_utilization_metric(&all_decisions, &current, thresholds),
        route_readiness_compliance_metric(&status, &route_scope, thresholds),
        receipt_completeness_metric(&all_posts, &current, thresholds),
        recurring_incident_metric(&all_incidents, &current),
    ];

    let drift = compute_drift(
        &all_incidents,
        &all_posts,
        &all_decisions,
        &current,
        &baseline,
        thresholds,
    );

    let findings = maturity_findings(&metrics, &drift);

    json!({
        "schema_version": 1,
        "mode": "dry_run",
        "mutates_external_systems": false,
        "redacted": true,
        "window": {
            "current": current.as_json(),
            "baseline": baseline.as_json(),
            "window_ms": window_ms,
        },
        "thresholds": thresholds,
        "metrics": metrics,
        "behavioral_drift": drift,
        "threshold_findings": findings,
        "counts": {
            "metrics": 6,
            "breached_metrics": metrics.iter().filter(|m| m["breached"] == json!(true)).count(),
            "drift_flags": drift.iter().filter(|d| d["drift_flagged"] == json!(true)).count(),
            "findings": findings.len(),
        },
    })
}

struct TimeWindow {
    from_ms: u64,
    to_ms: u64,
    label: &'static str,
}

impl TimeWindow {
    fn current(to_ms: u64, window_ms: u64) -> Self {
        Self {
            from_ms: to_ms.saturating_sub(window_ms),
            to_ms,
            label: "current",
        }
    }

    fn baseline(to_ms: u64, window_ms: u64) -> Self {
        let end = to_ms.saturating_sub(window_ms);
        Self {
            from_ms: end.saturating_sub(window_ms),
            to_ms: end,
            label: "baseline",
        }
    }

    fn contains(&self, timestamp_ms: u64) -> bool {
        timestamp_ms > self.from_ms && timestamp_ms <= self.to_ms
    }

    fn as_json(&self) -> Value {
        json!({ "from_ms": self.from_ms, "to_ms": self.to_ms, "label": self.label })
    }
}

/// Tenant-visible route topology for scoping the route-readiness metric. `None`
/// on either field means "count every enabled row" (local/implicit callers see
/// the full operator-global view); `Some(set)` restricts the count to that
/// tenant's own destinations / source projects.
struct RouteScope {
    destination_ids: Option<HashSet<String>>,
    source_project_ids: Option<HashSet<String>>,
}

impl RouteScope {
    fn includes_destination(&self, destination_id: &str) -> bool {
        self.destination_ids
            .as_ref()
            .is_none_or(|ids| ids.contains(destination_id))
    }

    fn includes_source(&self, project_id: &str) -> bool {
        self.source_project_ids
            .as_ref()
            .is_none_or(|ids| ids.contains(project_id))
    }
}

/// Compute the tenant-visible destination and source-project id sets, reusing
/// the same visibility rules the authority inventory applies (TAN-547) so route
/// readiness stays inside the tenant boundary and can't be skewed by another
/// tenant's topology.
async fn tenant_visible_route_scope(
    state: &AppState,
    tenant_context: &TenantContext,
) -> RouteScope {
    if tenant_context.is_local_implicit() {
        return RouteScope {
            destination_ids: None,
            source_project_ids: None,
        };
    }
    let config = state.incident_monitor_config().await;
    let visible_routes = config
        .routes
        .iter()
        .filter(|route| {
            crate::http::incident_monitor::incident_monitor_route_visible_to_tenant(
                route,
                tenant_context,
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let destination_ids =
        crate::http::incident_monitor::incident_monitor_tenant_referenced_destination_ids(
            &config,
            &visible_routes,
            tenant_context,
        );
    let source_project_ids = config
        .monitored_projects
        .iter()
        .filter(|project| {
            crate::http::incident_monitor::incident_monitor_entry_not_other_tenant(
                project.tenant_id.as_deref(),
                project.workspace_id.as_deref(),
                tenant_context,
            )
        })
        .map(|project| project.project_id.clone())
        .collect::<HashSet<String>>();
    RouteScope {
        destination_ids: Some(destination_ids),
        source_project_ids: Some(source_project_ids),
    }
}

fn is_high_risk_incident(incident: &IncidentMonitorIncidentRecord) -> bool {
    crate::incident_monitor::router::is_high_risk(incident.risk_level.as_deref())
}

fn metric(
    metric_id: &str,
    name: &str,
    numerator: u64,
    denominator: u64,
    missing_evidence_reasons: Vec<String>,
    evidence_refs: Vec<Value>,
    threshold: Option<f64>,
) -> Value {
    let rate = (denominator > 0).then(|| numerator as f64 / denominator as f64);
    // A metric with no data (denominator 0) can't breach — it's reported as
    // not-evaluable with its missing-evidence reasons instead.
    let breached = matches!((rate, threshold), (Some(rate), Some(min)) if rate < min);
    json!({
        "metric_id": metric_id,
        "name": name,
        "numerator": numerator,
        "denominator": denominator,
        "rate": rate,
        "threshold": threshold,
        "breached": breached,
        "evaluable": rate.is_some(),
        "missing_evidence_reasons": missing_evidence_reasons,
        "evidence_refs": evidence_refs,
    })
}

fn governance_confidence_metric(
    incidents: &[IncidentMonitorIncidentRecord],
    audited_incident_ids: &HashSet<String>,
    window: &TimeWindow,
    thresholds: &IncidentMonitorGovernanceThresholds,
) -> Value {
    let high_risk = incidents
        .iter()
        .filter(|incident| {
            window.contains(incident.updated_at_ms) && is_high_risk_incident(incident)
        })
        .collect::<Vec<_>>();
    let mut missing = Vec::new();
    let mut evidence = Vec::new();
    let mut complete = 0u64;
    for incident in &high_risk {
        let has_evidence = !incident.evidence_refs.is_empty();
        let has_trail = audited_incident_ids.contains(&incident.incident_id);
        if has_evidence && has_trail {
            complete += 1;
            evidence.push(json!({"kind": "incident", "id": incident.incident_id}));
        } else {
            let mut reasons = Vec::new();
            if !has_evidence {
                reasons.push("no evidence refs");
            }
            if !has_trail {
                reasons.push("no audited publish trail");
            }
            missing.push(format!(
                "incident {} lacks a complete audit trail ({})",
                incident.incident_id,
                reasons.join(", ")
            ));
        }
    }
    metric(
        "governance_confidence",
        "Governance confidence score (high-risk decisions with a complete audit trail)",
        complete,
        high_risk.len() as u64,
        missing,
        evidence,
        thresholds.min_for("governance_confidence"),
    )
}

fn authority_boundary_compliance_metric(
    decisions: &[tandem_types::PolicyDecisionRecord],
    window: &TimeWindow,
    thresholds: &IncidentMonitorGovernanceThresholds,
) -> Value {
    use tandem_types::PolicyDecisionEffect;
    let mut allow = 0u64;
    let mut deny = 0u64;
    let mut evidence = Vec::new();
    for decision in decisions
        .iter()
        .filter(|decision| window.contains(decision.created_at_ms))
    {
        match decision.decision {
            PolicyDecisionEffect::Allow => allow += 1,
            PolicyDecisionEffect::Deny => {
                deny += 1;
                evidence.push(json!({"kind": "policy_decision", "id": decision.decision_id}));
            }
            PolicyDecisionEffect::ApprovalRequired => {}
        }
    }
    let denominator = allow + deny;
    let missing = if denominator == 0 {
        vec!["no allow/deny policy decisions in window".to_string()]
    } else {
        Vec::new()
    };
    metric(
        "authority_boundary_compliance",
        "Authority boundary compliance rate (actions that stayed within configured authority)",
        allow,
        denominator,
        missing,
        evidence,
        thresholds.min_for("authority_boundary_compliance"),
    )
}

fn escalation_pathway_utilization_metric(
    decisions: &[tandem_types::PolicyDecisionRecord],
    window: &TimeWindow,
    thresholds: &IncidentMonitorGovernanceThresholds,
) -> Value {
    use tandem_types::PolicyDecisionEffect;
    let eligible = decisions
        .iter()
        .filter(|decision| {
            window.contains(decision.created_at_ms)
                && decision.decision == PolicyDecisionEffect::ApprovalRequired
        })
        .collect::<Vec<_>>();
    let mut reached = 0u64;
    let mut missing = Vec::new();
    let mut evidence = Vec::new();
    for decision in &eligible {
        if decision.approval_id.is_some() {
            reached += 1;
            evidence.push(json!({"kind": "policy_decision", "id": decision.decision_id}));
        } else {
            missing.push(format!(
                "approval-required decision {} never reached human review",
                decision.decision_id
            ));
        }
    }
    if eligible.is_empty() {
        missing.push("no escalation-eligible (approval-required) decisions in window".to_string());
    }
    metric(
        "escalation_pathway_utilization",
        "Escalation pathway utilization (escalation-eligible cases that reached human review)",
        reached,
        eligible.len() as u64,
        missing,
        evidence,
        thresholds.min_for("escalation_pathway_utilization"),
    )
}

fn route_readiness_compliance_metric(
    status: &crate::IncidentMonitorStatus,
    scope: &RouteScope,
    thresholds: &IncidentMonitorGovernanceThresholds,
) -> Value {
    let mut ready = 0u64;
    let mut total = 0u64;
    let mut missing = Vec::new();
    for destination in status
        .destination_readiness
        .iter()
        .filter(|row| row.enabled && scope.includes_destination(&row.destination_id))
    {
        total += 1;
        if destination.publish_ready {
            ready += 1;
        } else {
            missing.push(format!(
                "destination {} is not ready",
                destination.destination_id
            ));
        }
    }
    for source in status
        .source_readiness
        .iter()
        .filter(|row| row.enabled && scope.includes_source(&row.project_id))
    {
        total += 1;
        if source.ready {
            ready += 1;
        } else {
            let label = source
                .source_id
                .as_deref()
                .unwrap_or(source.project_id.as_str());
            missing.push(format!("source {label} is not ready"));
        }
    }
    if total == 0 {
        missing.push("no enabled sources or destinations configured".to_string());
    }
    metric(
        "route_readiness_compliance",
        "Route readiness compliance (enabled sources and destinations that are ready)",
        ready,
        total,
        missing,
        Vec::new(),
        thresholds.min_for("route_readiness_compliance"),
    )
}

fn receipt_completeness_metric(
    posts: &[IncidentMonitorPostRecord],
    window: &TimeWindow,
    thresholds: &IncidentMonitorGovernanceThresholds,
) -> Value {
    let considered = posts
        .iter()
        .filter(|post| window.contains(post.updated_at_ms) && post.status != "pending")
        .collect::<Vec<_>>();
    let mut complete = 0u64;
    let mut missing = Vec::new();
    for post in &considered {
        let posted = post.status == "posted";
        let has_reference = post.external_id.is_some() || post.external_url.is_some();
        if posted && has_reference {
            complete += 1;
        } else {
            missing.push(format!(
                "receipt {} incomplete (status={}, external_ref={})",
                post.post_id, post.status, has_reference
            ));
        }
    }
    if considered.is_empty() {
        missing.push("no completed publish receipts in window".to_string());
    }
    metric(
        "receipt_completeness",
        "Destination receipt completeness (published receipts with an external reference)",
        complete,
        considered.len() as u64,
        missing,
        Vec::new(),
        thresholds.min_for("receipt_completeness"),
    )
}

fn recurring_incident_metric(
    incidents: &[IncidentMonitorIncidentRecord],
    window: &TimeWindow,
) -> Value {
    let considered = incidents
        .iter()
        .filter(|incident| window.contains(incident.updated_at_ms))
        .collect::<Vec<_>>();
    let recurring = considered
        .iter()
        .filter(|incident| incident.occurrence_count > 1)
        .count() as u64;
    // Reported as a rate for visibility; no minimum threshold (recurrence is a
    // signal, not a pass/fail gate).
    metric(
        "recurring_incident_rate",
        "Recurring incident rate (incidents observed more than once)",
        recurring,
        considered.len() as u64,
        Vec::new(),
        Vec::new(),
        None,
    )
}

fn compute_drift(
    incidents: &[IncidentMonitorIncidentRecord],
    posts: &[IncidentMonitorPostRecord],
    decisions: &[tandem_types::PolicyDecisionRecord],
    current: &TimeWindow,
    baseline: &TimeWindow,
    thresholds: &IncidentMonitorGovernanceThresholds,
) -> Vec<Value> {
    use tandem_types::PolicyDecisionEffect;

    let publish_failure_rate = |window: &TimeWindow| {
        let considered = posts
            .iter()
            .filter(|post| window.contains(post.updated_at_ms) && post.status != "pending")
            .collect::<Vec<_>>();
        rate_of(
            considered
                .iter()
                .filter(|post| post.status == "failed")
                .count() as u64,
            considered.len() as u64,
        )
    };
    let denied_action_rate = |window: &TimeWindow| {
        let considered = decisions
            .iter()
            .filter(|decision| {
                window.contains(decision.created_at_ms)
                    && matches!(
                        decision.decision,
                        PolicyDecisionEffect::Allow | PolicyDecisionEffect::Deny
                    )
            })
            .collect::<Vec<_>>();
        rate_of(
            considered
                .iter()
                .filter(|decision| decision.decision == PolicyDecisionEffect::Deny)
                .count() as u64,
            considered.len() as u64,
        )
    };
    let escalation_rate = |window: &TimeWindow| {
        let considered = decisions
            .iter()
            .filter(|decision| window.contains(decision.created_at_ms))
            .collect::<Vec<_>>();
        rate_of(
            considered
                .iter()
                .filter(|decision| decision.decision == PolicyDecisionEffect::ApprovalRequired)
                .count() as u64,
            considered.len() as u64,
        )
    };

    let mut drift = vec![
        drift_row(
            "publish_failure_rate",
            publish_failure_rate(current),
            publish_failure_rate(baseline),
            thresholds.drift_rate_delta_max,
        ),
        drift_row(
            "denied_action_rate",
            denied_action_rate(current),
            denied_action_rate(baseline),
            thresholds.drift_rate_delta_max,
        ),
        drift_row(
            "escalation_rate",
            escalation_rate(current),
            escalation_rate(baseline),
            thresholds.drift_rate_delta_max,
        ),
    ];

    // Incident category distribution shift: flag any category whose share moved
    // by more than the drift delta between the two windows.
    let current_categories = category_shares(incidents, current);
    let baseline_categories = category_shares(incidents, baseline);
    let mut categories: Vec<String> = current_categories
        .keys()
        .chain(baseline_categories.keys())
        .cloned()
        .collect();
    categories.sort();
    categories.dedup();
    for category in categories {
        let current_share = current_categories.get(&category).copied().unwrap_or(0.0);
        let baseline_share = baseline_categories.get(&category).copied().unwrap_or(0.0);
        drift.push(drift_row(
            &format!("incident_category_share:{category}"),
            Some(current_share),
            Some(baseline_share),
            thresholds.drift_rate_delta_max,
        ));
    }

    drift
}

fn category_shares(
    incidents: &[IncidentMonitorIncidentRecord],
    window: &TimeWindow,
) -> BTreeMap<String, f64> {
    let considered = incidents
        .iter()
        .filter(|incident| window.contains(incident.updated_at_ms))
        .collect::<Vec<_>>();
    let total = considered.len() as f64;
    let mut counts = BTreeMap::<String, u64>::new();
    for incident in considered {
        let category = incident
            .risk_category
            .clone()
            .unwrap_or_else(|| "uncategorized".to_string());
        *counts.entry(category).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(category, count)| {
            let share = if total > 0.0 {
                count as f64 / total
            } else {
                0.0
            };
            (category, share)
        })
        .collect()
}

fn rate_of(numerator: u64, denominator: u64) -> Option<f64> {
    (denominator > 0).then(|| numerator as f64 / denominator as f64)
}

fn drift_row(signal: &str, current: Option<f64>, baseline: Option<f64>, delta_max: f64) -> Value {
    let delta = match (current, baseline) {
        (Some(current), Some(baseline)) => Some(current - baseline),
        _ => None,
    };
    let flagged = matches!(delta, Some(delta) if delta.abs() > delta_max);
    json!({
        "signal": signal,
        "current_rate": current,
        "baseline_rate": baseline,
        "delta": delta,
        "delta_max": delta_max,
        "drift_flagged": flagged,
        "evaluable": delta.is_some(),
    })
}

fn maturity_findings(metrics: &[Value], drift: &[Value]) -> Vec<Value> {
    let mut findings = Vec::new();
    for metric in metrics {
        if metric["breached"] == json!(true) {
            findings.push(json!({
                "kind": "metric_threshold_breach",
                "metric_id": metric["metric_id"],
                "rate": metric["rate"],
                "threshold": metric["threshold"],
                "severity": "high",
                "detail": format!(
                    "{} is below its minimum threshold",
                    metric["metric_id"].as_str().unwrap_or("metric")
                ),
                "dry_run": true,
                "incident_draft_suggestion": {
                    "source": "governance_maturity_metric",
                    "event_type": "security.governance_metric.breached",
                    "risk_level": "high",
                    "risk_category": "governance_gap",
                    "route_tags": ["governance_maturity", metric["metric_id"].clone()],
                },
            }));
        }
    }
    for row in drift {
        if row["drift_flagged"] == json!(true) {
            findings.push(json!({
                "kind": "behavioral_drift",
                "signal": row["signal"],
                "delta": row["delta"],
                "delta_max": row["delta_max"],
                "severity": "medium",
                "detail": format!(
                    "{} drifted beyond the tolerated delta",
                    row["signal"].as_str().unwrap_or("signal")
                ),
                "dry_run": true,
            }));
        }
    }
    findings
}
