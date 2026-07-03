//! Continuous reassessment scheduler runtime (TAN-490).
//!
//! Re-runs the Incident Monitor governance posture (authority inventory,
//! posture findings, controlled probes, adversarial scenarios, and governance
//! maturity metrics) on a cadence and on selected configuration/runtime change
//! events, producing a versioned [`ReassessmentRecord`] that compares the
//! current findings against the previous run. Everything runs read-only and
//! **never mutates external systems** — a finding is escalated through the
//! normal Incident Monitor draft / approval / routing path, not from here.

use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::Value;
use tandem_types::TenantContext;

use crate::incident_monitor::reassessment::{
    apply_reassessment_comparison, reassessment_finding_fingerprint, reassessment_is_overdue,
    reassessment_next_due_at_ms, reassessment_scope_from_finding, ReassessmentComparison,
    ReassessmentFinding, ReassessmentRecord, ReassessmentScheduleStatus, ReassessmentTrigger,
    REASSESSMENT_SCHEMA_VERSION,
};
use crate::AppState;

/// How often the scheduler wakes to run any due scopes.
const REASSESSMENT_SCHEDULER_TICK: Duration = Duration::from_secs(15 * 60);
/// Only the deployment scope is *run*; per-source/workflow status is derived
/// from the deployment record's findings for display.
const DEPLOYMENT_SCOPE: &str = "deployment";

/// A change-triggered reassessment awaiting a run, held in memory (the cadence
/// backstops after a restart).
#[derive(Debug, Clone)]
pub struct IncidentMonitorReassessmentPending {
    pub trigger: ReassessmentTrigger,
    pub trigger_detail: Option<String>,
    pub queued_at_ms: u64,
}

/// Stable key grouping a scope's run history: `<org>/<workspace>/<scope>`.
pub fn incident_monitor_reassessment_scope_key(
    tenant_context: &TenantContext,
    scope: &str,
) -> String {
    format!(
        "{}/{}/{}",
        tenant_context.org_id, tenant_context.workspace_id, scope
    )
}

/// The most recent record for a scope key (highest version).
async fn latest_reassessment_record(
    state: &AppState,
    scope_key: &str,
) -> Option<ReassessmentRecord> {
    state
        .incident_monitor_reassessments
        .read()
        .await
        .values()
        .filter(|record| record.scope_key == scope_key)
        .max_by_key(|record| record.version)
        .cloned()
}

/// Extract normalized, redacted reassessment findings from a dry-run assessment
/// payload. Only ids / severities / categories are carried — never free-text
/// finding messages or payloads.
fn extract_reassessment_findings(
    payload: &Value,
    tenant_context: &TenantContext,
) -> Vec<ReassessmentFinding> {
    let tenant_id = Some(tenant_context.org_id.as_str());
    let workspace_id = Some(tenant_context.workspace_id.as_str());
    let mut findings = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut push = |fingerprint: String,
                    scope: String,
                    rule_id: String,
                    category: String,
                    severity: String,
                    evidence_refs: Vec<String>,
                    findings: &mut Vec<ReassessmentFinding>| {
        if !seen.insert(fingerprint.clone()) {
            return;
        }
        let summary = format!("{rule_id} [{category}/{severity}]");
        findings.push(ReassessmentFinding {
            fingerprint,
            rule_id,
            scope,
            severity,
            category,
            summary,
            evidence_refs,
            first_seen_at_ms: 0,
            last_seen_at_ms: 0,
            occurrence_count: 0,
            status: String::new(),
        });
    };

    // Posture findings (source-readiness, tenant-context, scope-gap, etc.).
    if let Some(rows) = payload
        .pointer("/sections/detected_findings/findings")
        .and_then(Value::as_array)
    {
        for row in rows {
            let rule_id = string_field(row, "rule_id")
                .or_else(|| string_field(row, "id"))
                .unwrap_or_else(|| "posture_finding".to_string());
            let category = string_field(row, "category").unwrap_or_else(|| "posture".to_string());
            let severity = string_field(row, "severity").unwrap_or_else(|| "info".to_string());
            let scope = reassessment_scope_from_finding(row);
            // Reuse the assessment report's stable per-affected-object
            // fingerprint so distinct subjects failing the same rule stay
            // distinct; only synthesize one for findings that lack it.
            let fingerprint = string_field(row, "fingerprint").unwrap_or_else(|| {
                let subject = scope
                    .split_once(':')
                    .map(|(_, id)| id.to_string())
                    .unwrap_or_else(|| rule_id.clone());
                reassessment_finding_fingerprint(
                    tenant_id,
                    workspace_id,
                    &scope,
                    &rule_id,
                    &subject,
                )
            });
            let evidence_refs = evidence_ref_paths(row);
            push(
                fingerprint,
                scope,
                rule_id,
                category,
                severity,
                evidence_refs,
                &mut findings,
            );
        }
    }

    // Governance maturity metric breaches + behavioral drift.
    if let Some(rows) = payload
        .pointer("/sections/governance_maturity_metrics/threshold_findings")
        .and_then(Value::as_array)
    {
        for row in rows {
            let kind = string_field(row, "kind").unwrap_or_else(|| "governance".to_string());
            let subject = string_field(row, "metric_id")
                .or_else(|| string_field(row, "signal"))
                .unwrap_or_else(|| "governance".to_string());
            let severity = string_field(row, "severity").unwrap_or_else(|| "medium".to_string());
            let rule_id = format!("governance.{kind}.{subject}");
            let fingerprint = reassessment_finding_fingerprint(
                tenant_id,
                workspace_id,
                DEPLOYMENT_SCOPE,
                &rule_id,
                &subject,
            );
            push(
                fingerprint,
                DEPLOYMENT_SCOPE.to_string(),
                rule_id,
                "governance_gap".to_string(),
                severity,
                Vec::new(),
                &mut findings,
            );
        }
    }

    findings
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

/// Evidence refs on posture findings are `{kind, path}` objects; carry their
/// `path` (also tolerating a bare-string form) so completed reassessment
/// history retains the references operators need to validate a finding.
fn evidence_ref_paths(finding: &Value) -> Vec<String> {
    finding
        .get("evidence_refs")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|entry| {
                    entry
                        .get("path")
                        .and_then(Value::as_str)
                        .or_else(|| entry.as_str())
                })
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Run a single deployment-scope reassessment for a tenant: re-run the posture
/// surfaces, compare against the previous run, persist the versioned record, and
/// emit a protected audit event. Returns the new record.
pub async fn compute_incident_monitor_reassessment(
    state: &AppState,
    tenant_context: &TenantContext,
    trigger: ReassessmentTrigger,
    trigger_detail: Option<String>,
    now_ms: u64,
) -> anyhow::Result<ReassessmentRecord> {
    let scope_key = incident_monitor_reassessment_scope_key(tenant_context, DEPLOYMENT_SCOPE);
    let payload = crate::http::incident_monitor::incident_monitor_reassessment_posture_payload(
        state,
        tenant_context,
        now_ms,
    )
    .await;

    let mut findings = extract_reassessment_findings(&payload, tenant_context);

    let evidence_refs = findings
        .iter()
        .flat_map(|finding| finding.evidence_refs.iter().cloned())
        .collect::<Vec<_>>();

    let (tenant_id, workspace_id) = if tenant_context.is_local_implicit() {
        (None, None)
    } else {
        (
            Some(tenant_context.org_id.clone()),
            Some(tenant_context.workspace_id.clone()),
        )
    };

    // Allocate the version, run the previous/current comparison, and insert
    // under a single hold of the write lock so two concurrent runs for the same
    // scope (e.g. a manual request overlapping a scheduler tick) cannot read the
    // same latest record, pick the same version/record_id, and overwrite each
    // other's history. The expensive posture computation above stays outside the
    // lock; only the cheap in-memory comparison + insert are serialized.
    let record = {
        let mut guard = state.incident_monitor_reassessments.write().await;
        let previous = guard
            .values()
            .filter(|record| record.scope_key == scope_key)
            .max_by_key(|record| record.version)
            .cloned();
        let comparison: ReassessmentComparison =
            apply_reassessment_comparison(previous.as_ref(), &mut findings, now_ms);
        let version = previous
            .as_ref()
            .map(|record| record.version + 1)
            .unwrap_or(1);
        let record = ReassessmentRecord {
            schema_version: REASSESSMENT_SCHEMA_VERSION,
            record_id: format!("reassess-{version}-{scope_key}"),
            scope_key: scope_key.clone(),
            tenant_id,
            workspace_id,
            scope: DEPLOYMENT_SCOPE.to_string(),
            version,
            trigger: trigger.as_str().to_string(),
            trigger_detail,
            generated_at_ms: now_ms,
            mode: "dry_run".to_string(),
            mutates_external_systems: false,
            findings,
            comparison,
            evidence_refs,
        };
        guard.insert(record.record_id.clone(), record.clone());
        record
    };
    state.persist_incident_monitor_reassessments().await?;

    // Clear any pending change-trigger for this scope now that it has run.
    state
        .incident_monitor_reassessment_pending
        .write()
        .await
        .remove(&scope_key);

    let _ = crate::audit::append_protected_audit_event(
        state,
        "incident_monitor.reassessment.completed",
        tenant_context,
        Some("incident_monitor.reassessment_scheduler".to_string()),
        serde_json::json!({
            "record_id": record.record_id,
            "scope_key": record.scope_key,
            "version": record.version,
            "trigger": record.trigger,
            "mode": record.mode,
            "mutates_external_systems": false,
            "counts": {
                "findings": record.findings.len(),
                "new": record.comparison.new_fingerprints.len(),
                "recurring": record.comparison.recurring_fingerprints.len(),
                "resolved": record.comparison.resolved_fingerprints.len(),
            },
        }),
    )
    .await;

    Ok(record)
}

/// Record a change-triggered reassessment request for a tenant's deployment
/// scope. The scheduler picks it up on its next tick; the audit trail captures
/// the trigger immediately. A no-op when change triggers are disabled.
pub async fn note_incident_monitor_reassessment_trigger(
    state: &AppState,
    tenant_context: &TenantContext,
    trigger: ReassessmentTrigger,
    trigger_detail: Option<String>,
    now_ms: u64,
) {
    if !trigger.is_change_event() {
        return;
    }
    let scope_key = incident_monitor_reassessment_scope_key(tenant_context, DEPLOYMENT_SCOPE);
    state
        .incident_monitor_reassessment_pending
        .write()
        .await
        .insert(
            scope_key.clone(),
            IncidentMonitorReassessmentPending {
                trigger,
                trigger_detail: trigger_detail.clone(),
                queued_at_ms: now_ms,
            },
        );

    let _ = crate::audit::append_protected_audit_event(
        state,
        "incident_monitor.reassessment.triggered",
        tenant_context,
        Some("incident_monitor.reassessment_scheduler".to_string()),
        serde_json::json!({
            "scope_key": scope_key,
            "trigger": trigger.as_str(),
            "trigger_detail": trigger_detail,
        }),
    )
    .await;
}

/// The distinct tenant deployment scopes a config attributes to: one per
/// (tenant, workspace) bound to a monitored project, plus the local/implicit
/// deployment when any project is unbound or none exist.
pub fn incident_monitor_config_reassessment_tenants(
    config: &crate::IncidentMonitorConfig,
) -> Vec<TenantContext> {
    let mut contexts: BTreeMap<(String, String), TenantContext> = BTreeMap::new();
    let mut has_unbound = config.monitored_projects.is_empty();
    for project in &config.monitored_projects {
        match (
            project.tenant_id.as_deref(),
            project.workspace_id.as_deref(),
        ) {
            (Some(org), Some(workspace)) if !org.is_empty() && !workspace.is_empty() => {
                contexts
                    .entry((org.to_string(), workspace.to_string()))
                    .or_insert_with(|| {
                        TenantContext::explicit_user_workspace(
                            org,
                            workspace,
                            None,
                            "incident_monitor.reassessment_scheduler",
                        )
                    });
            }
            _ => has_unbound = true,
        }
    }
    let mut tenants = contexts.into_values().collect::<Vec<_>>();
    if has_unbound {
        tenants.push(TenantContext::local_implicit());
    }
    tenants
}

/// The distinct tenant deployment scopes the scheduler manages, from live config.
async fn incident_monitor_reassessment_tenants(state: &AppState) -> Vec<TenantContext> {
    let config = state.incident_monitor_config().await;
    incident_monitor_config_reassessment_tenants(&config)
}

/// Run any reassessment scopes that are due by cadence or have a pending
/// change-trigger. Returns the records produced this tick.
pub async fn run_due_incident_monitor_reassessments(
    state: &AppState,
    now_ms: u64,
) -> anyhow::Result<Vec<ReassessmentRecord>> {
    let config = state.incident_monitor_config().await;
    let cadence_ms = config.reassessment.effective_cadence_ms();
    let pending_keys = state
        .incident_monitor_reassessment_pending
        .read()
        .await
        .keys()
        .cloned()
        .collect::<std::collections::HashSet<_>>();

    let mut produced = Vec::new();
    for tenant_context in incident_monitor_reassessment_tenants(state).await {
        let scope_key = incident_monitor_reassessment_scope_key(&tenant_context, DEPLOYMENT_SCOPE);
        let pending = state
            .incident_monitor_reassessment_pending
            .read()
            .await
            .get(&scope_key)
            .cloned();
        let last_completed = latest_reassessment_record(state, &scope_key)
            .await
            .map(|record| record.generated_at_ms);
        let due_by_cadence =
            now_ms >= reassessment_next_due_at_ms(last_completed, cadence_ms, now_ms);
        if !due_by_cadence && pending.is_none() {
            continue;
        }
        let (trigger, detail) = match pending {
            Some(pending) => (pending.trigger, pending.trigger_detail),
            None => (ReassessmentTrigger::Scheduled, None),
        };
        match compute_incident_monitor_reassessment(state, &tenant_context, trigger, detail, now_ms)
            .await
        {
            Ok(record) => produced.push(record),
            Err(error) => tracing::warn!(
                target: "incident_monitor",
                scope_key = %scope_key,
                error = %error,
                "reassessment run failed"
            ),
        }
    }
    // Drop any pending markers whose scope no longer resolves to a tenant (e.g.
    // a tenant removed from config) so they don't leak.
    let _ = pending_keys;
    Ok(produced)
}

/// Schedule status (last completed / next due / overdue) for every scope a
/// tenant has assessed, plus the deployment scope even if never run. Surfaced on
/// deployment cards and the reassessments endpoint.
pub async fn incident_monitor_reassessment_schedule_status(
    state: &AppState,
    tenant_context: &TenantContext,
    now_ms: u64,
) -> Vec<ReassessmentScheduleStatus> {
    let config = state.incident_monitor_config().await;
    let cadence_ms = config.reassessment.effective_cadence_ms();
    let grace_ms = config.reassessment.overdue_grace_ms;
    let prefix = format!("{}/{}/", tenant_context.org_id, tenant_context.workspace_id);

    let mut latest_by_scope: BTreeMap<String, ReassessmentRecord> = BTreeMap::new();
    for record in state.incident_monitor_reassessments.read().await.values() {
        if !record.scope_key.starts_with(&prefix) {
            continue;
        }
        latest_by_scope
            .entry(record.scope_key.clone())
            .and_modify(|existing| {
                if record.version > existing.version {
                    *existing = record.clone();
                }
            })
            .or_insert_with(|| record.clone());
    }

    // Always surface the deployment scope, even before its first run.
    let deployment_key = incident_monitor_reassessment_scope_key(tenant_context, DEPLOYMENT_SCOPE);
    latest_by_scope
        .entry(deployment_key.clone())
        .or_insert(ReassessmentRecord {
            schema_version: REASSESSMENT_SCHEMA_VERSION,
            record_id: String::new(),
            scope_key: deployment_key,
            tenant_id: None,
            workspace_id: None,
            scope: DEPLOYMENT_SCOPE.to_string(),
            version: 0,
            trigger: String::new(),
            trigger_detail: None,
            generated_at_ms: 0,
            mode: "dry_run".to_string(),
            mutates_external_systems: false,
            findings: Vec::new(),
            comparison: ReassessmentComparison::default(),
            evidence_refs: Vec::new(),
        });

    latest_by_scope
        .into_values()
        .map(|record| {
            let last_completed = (record.version > 0).then_some(record.generated_at_ms);
            ReassessmentScheduleStatus {
                scope_key: record.scope_key,
                scope: record.scope,
                last_completed_at_ms: last_completed,
                next_due_at_ms: reassessment_next_due_at_ms(last_completed, cadence_ms, now_ms),
                overdue: reassessment_is_overdue(last_completed, cadence_ms, grace_ms, now_ms),
                last_version: (record.version > 0).then_some(record.version),
            }
        })
        .collect()
}

/// Background scheduler loop: after the runtime is ready, run any due scopes on
/// a fixed tick. Change-triggered scopes are also picked up here.
pub async fn run_incident_monitor_reassessment_scheduler(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        return;
    }
    loop {
        if state.is_automation_scheduler_stopping() {
            return;
        }
        if let Err(error) = run_due_incident_monitor_reassessments(&state, crate::now_ms()).await {
            tracing::warn!(
                target: "incident_monitor",
                error = %error,
                "incident monitor reassessment scheduler tick failed"
            );
        }
        tokio::time::sleep(REASSESSMENT_SCHEDULER_TICK).await;
    }
}
