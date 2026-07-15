// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

use crate::audit::{load_protected_audit_events_for_tenant, ProtectedAuditEnvelope};
use crate::stateful_runtime::{
    list_stateful_compensations, list_stateful_dead_letters, list_stateful_outbox,
    list_stateful_run_snapshots as list_snapshot_records, list_stateful_tool_effects,
    list_stateful_waits, query_stateful_run_events,
    stateful_reliability_path_from_runtime_events_path, stateful_run_from_automation_v2,
    stateful_run_from_workflow, StatefulCompensationRecord, StatefulCompensationStatus,
    StatefulDeadLetterRecord, StatefulDeadLetterStatus, StatefulOutboxRecord, StatefulOutboxStatus,
    StatefulReliabilityQuery, StatefulRunEventQuery, StatefulRunEventRecord,
    StatefulRuntimeStoragePaths, StatefulToolEffectRecord, StatefulToolEffectStatus,
    StatefulWaitQuery, StatefulWaitRecord, StatefulWaitStatus, StatefulWorkflowRunRecord,
    StatefulWorkflowRunStatus, STATEFUL_RUNTIME_SCHEMA_VERSION,
};
use tandem_types::{PolicyDecisionEffect, PolicyDecisionRecord};

const DEFAULT_OBSERVABILITY_LIMIT: usize = 100;
const MAX_OBSERVABILITY_LIMIT: usize = 500;

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunObservabilityQuery {
    pub event_limit: Option<usize>,
    pub snapshot_limit: Option<usize>,
    pub reliability_limit: Option<usize>,
    pub audit_limit: Option<usize>,
}

pub(super) async fn get_stateful_run_observability(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<StatefulRunObservabilityQuery>,
) -> Response {
    let Some(run) = find_visible_stateful_run(&state, &tenant_context, &run_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "stateful_run_not_found",
                "run_id": run_id,
            })),
        )
            .into_response();
    };

    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let event_limit = limit(query.event_limit);
    let snapshot_limit = limit(query.snapshot_limit);
    let reliability_limit = limit(query.reliability_limit);
    let audit_limit = limit(query.audit_limit);
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let reliability_query = StatefulReliabilityQuery {
        run_id: Some(&run_id),
        status: None,
        source_type: None,
        after_id: None,
        before_created_at_ms: None,
        active_recovery_only: false,
        limit: Some(reliability_limit),
    };
    let waits = list_stateful_waits(
        &paths.waits_path,
        &tenant_context,
        StatefulWaitQuery {
            run_id: Some(&run_id),
            wait_kind: None,
            status: None,
            limit: Some(reliability_limit),
        },
    );
    let events = query_stateful_run_events(
        &paths.run_events_path,
        &tenant_context,
        StatefulRunEventQuery {
            run_id: &run_id,
            after_seq: None,
            before_seq: None,
            limit: Some(event_limit),
            tail: true,
        },
    );
    let snapshots = list_snapshot_records(
        &paths.snapshots_root,
        &tenant_context,
        &run_id,
        Some(snapshot_limit),
    );
    let outbox = list_stateful_outbox(&reliability_path, &tenant_context, reliability_query);
    let tool_effects =
        list_stateful_tool_effects(&reliability_path, &tenant_context, reliability_query);
    let dead_letters =
        list_stateful_dead_letters(&reliability_path, &tenant_context, reliability_query);
    let compensations =
        list_stateful_compensations(&reliability_path, &tenant_context, reliability_query);
    let active_recovery_query = StatefulReliabilityQuery {
        active_recovery_only: true,
        ..reliability_query
    };
    let active_dead_letters =
        list_stateful_dead_letters(&reliability_path, &tenant_context, active_recovery_query);
    let active_compensations =
        list_stateful_compensations(&reliability_path, &tenant_context, active_recovery_query);
    let policy_decisions = state
        .list_policy_decisions_for_run(&tenant_context, &run_id, reliability_limit)
        .await;
    let protected_audit = load_protected_audit_events_for_tenant(&state, &tenant_context)
        .await
        .into_iter()
        .filter(|event| audit_event_mentions_run(event, &run_id, &policy_decisions))
        .collect::<Vec<_>>();
    let protected_audit = tail_protected_audit_events(protected_audit, audit_limit);

    let active_waits = active_waits_for_observability(&run, &waits);
    let blocking_reasons = blocking_reasons(
        &run,
        &active_waits,
        &policy_decisions,
        &outbox,
        &tool_effects,
        &active_dead_letters,
        &active_compensations,
    );
    let operator_actions = operator_actions(
        &run,
        &active_waits,
        &policy_decisions,
        &tool_effects,
        &active_dead_letters,
        &active_compensations,
    );
    let event_correlation_ids = event_correlation_ids(&events);
    let latest_event = events.last().map(event_summary);
    let latest_snapshot = snapshots.last().map(|snapshot| {
        json!({
            "snapshot_id": &snapshot.snapshot_id,
            "seq": snapshot.seq,
            "status": &snapshot.status,
            "phase": &snapshot.phase,
            "created_at_ms": snapshot.created_at_ms,
        })
    });

    Json(json!({
        "run_id": run_id,
        "source": "stateful_runtime_observability",
        "run": run,
        "current_wait": active_waits.first(),
        "waits": waits,
        "policy_decisions": policy_decisions.iter().map(policy_decision_summary).collect::<Vec<_>>(),
        "reliability": {
            "outbox": outbox,
            "tool_effects": tool_effects,
            "dead_letters": dead_letters,
            "compensations": compensations,
        },
        "audit": {
            "protected_events": protected_audit.iter().map(protected_audit_summary).collect::<Vec<_>>(),
            "payload_policy": "redacted_payload_digest_only",
        },
        "events": {
            "latest": latest_event,
            "tail": events,
            "correlation_ids": event_correlation_ids,
        },
        "snapshots": {
            "latest": latest_snapshot,
            "items": snapshots,
        },
        "operator_summary": {
            "is_blocked": !blocking_reasons.is_empty(),
            "blocking_reasons": blocking_reasons,
            "allowed_actions": operator_actions,
        },
        "counts": {
            "waits": waits.len(),
            "active_waits": active_waits.len(),
            "policy_decisions": policy_decisions.len(),
            "outbox": outbox.len(),
            "tool_effects": tool_effects.len(),
            "dead_letters": dead_letters.len(),
            "compensations": compensations.len(),
            "protected_audit_events": protected_audit.len(),
            "events": events.len(),
            "snapshots": snapshots.len(),
        },
        "limits": {
            "event_limit": event_limit,
            "snapshot_limit": snapshot_limit,
            "reliability_limit": reliability_limit,
            "audit_limit": audit_limit,
        },
    }))
    .into_response()
}

async fn find_visible_stateful_run(
    state: &AppState,
    tenant_context: &TenantContext,
    run_id: &str,
) -> Option<StatefulWorkflowRunRecord> {
    let automation_runs = state.automation_v2_runs.read().await;
    if let Some(run) = automation_runs.get(run_id) {
        let stateful = stateful_run_from_automation_v2(run);
        if stateful.scope.visible_to_tenant(tenant_context) {
            return Some(stateful);
        }
    }
    drop(automation_runs);

    let workflow_runs = state.workflow_runs.read().await;
    workflow_runs.get(run_id).and_then(|run| {
        let stateful = stateful_run_from_workflow(run);
        stateful
            .scope
            .visible_to_tenant(tenant_context)
            .then_some(stateful)
    })
}

fn limit(value: Option<usize>) -> usize {
    value
        .unwrap_or(DEFAULT_OBSERVABILITY_LIMIT)
        .clamp(1, MAX_OBSERVABILITY_LIMIT)
}

fn active_waits_for_observability(
    run: &StatefulWorkflowRunRecord,
    waits: &[StatefulWaitRecord],
) -> Vec<StatefulWaitRecord> {
    let mut active_waits = waits
        .iter()
        .filter(|wait| wait.status.is_active())
        .cloned()
        .collect::<Vec<_>>();
    if let Some(inferred_wait) = inferred_active_wait_from_run(run, &active_waits) {
        active_waits.push(inferred_wait);
    }
    active_waits
}

fn inferred_active_wait_from_run(
    run: &StatefulWorkflowRunRecord,
    active_waits: &[StatefulWaitRecord],
) -> Option<StatefulWaitRecord> {
    let wait_kind = run.active_wait_kind.clone()?;
    let wait_id = run.active_wait_id.as_ref()?;
    if active_waits.iter().any(|wait| wait.wait_id == *wait_id) {
        return None;
    }
    Some(StatefulWaitRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        wait_id: wait_id.clone(),
        run_id: run.run_id.clone(),
        wait_kind,
        status: StatefulWaitStatus::Waiting,
        scope: run.scope.clone(),
        phase_id: run.current_phase_id.clone(),
        reason: Some("run_level_active_wait".to_string()),
        created_at_ms: run.updated_at_ms,
        updated_at_ms: run.updated_at_ms,
        wake_at_ms: None,
        timeout_policy: None,
        event_seq: None,
        wake_idempotency_key: None,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        completed_at_ms: None,
        metadata: Some(json!({ "source": "run_record" })),
    })
}

fn tail_protected_audit_events(
    mut events: Vec<ProtectedAuditEnvelope>,
    limit: usize,
) -> Vec<ProtectedAuditEnvelope> {
    let start = events.len().saturating_sub(limit);
    events.split_off(start)
}

fn blocking_reasons(
    run: &StatefulWorkflowRunRecord,
    active_waits: &[StatefulWaitRecord],
    policy_decisions: &[PolicyDecisionRecord],
    outbox: &[StatefulOutboxRecord],
    tool_effects: &[StatefulToolEffectRecord],
    dead_letters: &[StatefulDeadLetterRecord],
    compensations: &[StatefulCompensationRecord],
) -> Vec<Value> {
    let mut reasons = Vec::new();
    if run.status.needs_operator_attention() {
        reasons.push(json!({
            "kind": "run_status",
            "status": &run.status,
            "phase_id": &run.current_phase_id,
        }));
    }
    for wait in active_waits {
        reasons.push(json!({
            "kind": "active_wait",
            "wait_id": &wait.wait_id,
            "wait_kind": &wait.wait_kind,
            "status": &wait.status,
            "phase_id": &wait.phase_id,
            "reason": &wait.reason,
        }));
    }
    for decision in policy_decisions {
        if matches!(
            decision.decision,
            PolicyDecisionEffect::Deny | PolicyDecisionEffect::ApprovalRequired
        ) {
            reasons.push(json!({
                "kind": "policy_decision",
                "decision_id": &decision.decision_id,
                "decision": &decision.decision,
                "reason_code": &decision.reason_code,
                "tool": &decision.tool,
                "approval_id": &decision.approval_id,
            }));
        }
    }
    for row in outbox {
        if matches!(
            row.status,
            StatefulOutboxStatus::Pending
                | StatefulOutboxStatus::Failed
                | StatefulOutboxStatus::DeadLettered
        ) {
            reasons.push(json!({
                "kind": "outbox",
                "outbox_id": &row.outbox_id,
                "status": &row.status,
                "operation": &row.operation,
                "dead_letter_id": &row.dead_letter_id,
            }));
        }
    }
    for effect in tool_effects {
        if matches!(
            effect.status,
            StatefulToolEffectStatus::Failed | StatefulToolEffectStatus::Unknown
        ) {
            reasons.push(json!({
                "kind": "tool_effect",
                "effect_id": &effect.effect_id,
                "status": &effect.status,
                "operation": &effect.operation,
                "compensation_id": &effect.compensation_id,
            }));
        }
    }
    for row in dead_letters {
        if row.status == StatefulDeadLetterStatus::Open {
            reasons.push(json!({
                "kind": "dead_letter",
                "dead_letter_id": &row.dead_letter_id,
                "source_type": &row.source_type,
                "reason": &row.reason,
            }));
        }
    }
    for row in compensations {
        if matches!(
            row.status,
            StatefulCompensationStatus::Proposed
                | StatefulCompensationStatus::AwaitingApproval
                | StatefulCompensationStatus::Failed
        ) {
            reasons.push(json!({
                "kind": "compensation",
                "compensation_id": &row.compensation_id,
                "status": &row.status,
                "target_effect_id": &row.target_effect_id,
                "approval_required": row.approval_required,
            }));
        }
    }
    reasons
}

fn operator_actions(
    run: &StatefulWorkflowRunRecord,
    active_waits: &[StatefulWaitRecord],
    policy_decisions: &[PolicyDecisionRecord],
    tool_effects: &[StatefulToolEffectRecord],
    dead_letters: &[StatefulDeadLetterRecord],
    compensations: &[StatefulCompensationRecord],
) -> Vec<Value> {
    let mut actions = Vec::new();
    if run.status.needs_operator_attention() {
        actions.push(json!({
            "action": "resume_from_checkpoint",
            "requires_status": ["paused", "blocked", "failed", "dead_lettered", "retrying"],
        }));
    }
    if !active_waits.is_empty() {
        actions.push(json!({
            "action": "review_wait",
            "wait_ids": active_waits.iter().map(|wait| wait.wait_id.as_str()).collect::<Vec<_>>(),
        }));
    }
    let approvals = policy_decisions
        .iter()
        .filter(|decision| decision.decision == PolicyDecisionEffect::ApprovalRequired)
        .filter_map(|decision| {
            decision
                .approval_id
                .as_deref()
                .or(Some(decision.decision_id.as_str()))
        })
        .collect::<Vec<_>>();
    if !approvals.is_empty() {
        actions.push(json!({
            "action": "approve_or_deny_policy_decision",
            "approval_refs": approvals,
        }));
    }
    let failed_effects = tool_effects
        .iter()
        .filter(|effect| {
            matches!(
                effect.status,
                StatefulToolEffectStatus::Failed | StatefulToolEffectStatus::Unknown
            )
        })
        .map(|effect| effect.effect_id.as_str())
        .collect::<Vec<_>>();
    if !failed_effects.is_empty() {
        actions.push(json!({
            "action": "retry_or_reconcile_effect",
            "effect_ids": failed_effects,
        }));
    }
    let open_dead_letters = dead_letters
        .iter()
        .filter(|row| row.status == StatefulDeadLetterStatus::Open)
        .map(|row| row.dead_letter_id.as_str())
        .collect::<Vec<_>>();
    if !open_dead_letters.is_empty() {
        actions.push(json!({
            "action": "resolve_dead_letter",
            "dead_letter_ids": open_dead_letters,
        }));
    }
    let pending_compensations = compensations
        .iter()
        .filter(|row| {
            matches!(
                row.status,
                StatefulCompensationStatus::Proposed | StatefulCompensationStatus::AwaitingApproval
            )
        })
        .map(|row| row.compensation_id.as_str())
        .collect::<Vec<_>>();
    if !pending_compensations.is_empty() {
        actions.push(json!({
            "action": "run_or_cancel_compensation",
            "compensation_ids": pending_compensations,
        }));
    }
    actions
}

fn policy_decision_summary(decision: &PolicyDecisionRecord) -> Value {
    json!({
        "decision_id": &decision.decision_id,
        "run_id": &decision.run_id,
        "automation_id": &decision.automation_id,
        "node_id": &decision.node_id,
        "tool": &decision.tool,
        "decision": &decision.decision,
        "reason_code": &decision.reason_code,
        "risk_tier": &decision.risk_tier,
        "policy_id": &decision.policy_id,
        "grant_id": &decision.grant_id,
        "approval_id": &decision.approval_id,
        "audit_event_id": &decision.audit_event_id,
        "created_at_ms": decision.created_at_ms,
    })
}

fn protected_audit_summary(event: &ProtectedAuditEnvelope) -> Value {
    let payload = serde_json::to_string(&event.payload).unwrap_or_default();
    json!({
        "event_id": &event.event_id,
        "event_type": &event.event_type,
        "created_at_ms": event.created_at_ms,
        "seq": event.seq,
        "record_hash": &event.record_hash,
        "actor": &event.actor,
        "payload_digest": format!("sha256:{}", crate::sha256_hex(&[payload.as_str()])),
    })
}

fn event_summary(event: &StatefulRunEventRecord) -> Value {
    json!({
        "event_id": &event.event_id,
        "seq": event.seq,
        "event_type": &event.event_type,
        "occurred_at_ms": event.occurred_at_ms,
        "phase_id": &event.phase_id,
        "causation_id": &event.causation_id,
        "correlation_id": &event.correlation_id,
    })
}

fn event_correlation_ids(events: &[StatefulRunEventRecord]) -> Vec<Value> {
    events
        .iter()
        .filter_map(|event| {
            let causation_id = event.causation_id.as_deref();
            let correlation_id = event.correlation_id.as_deref();
            (causation_id.is_some() || correlation_id.is_some()).then(|| {
                json!({
                    "event_id": &event.event_id,
                    "seq": event.seq,
                    "causation_id": causation_id,
                    "correlation_id": correlation_id,
                })
            })
        })
        .collect()
}

fn audit_event_mentions_run(
    event: &ProtectedAuditEnvelope,
    run_id: &str,
    policy_decisions: &[PolicyDecisionRecord],
) -> bool {
    value_contains_string(&event.payload, run_id)
        || policy_decisions
            .iter()
            .any(|decision| value_contains_string(&event.payload, &decision.decision_id))
}

fn value_contains_string(value: &Value, needle: &str) -> bool {
    match value {
        Value::String(value) => value == needle,
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_string(value, needle)),
        Value::Object(values) => values
            .values()
            .any(|value| value_contains_string(value, needle)),
        _ => false,
    }
}

trait StatefulStatusExt {
    fn needs_operator_attention(&self) -> bool;
}

impl StatefulStatusExt for StatefulWorkflowRunStatus {
    fn needs_operator_attention(&self) -> bool {
        matches!(
            self,
            StatefulWorkflowRunStatus::Paused
                | StatefulWorkflowRunStatus::Blocked
                | StatefulWorkflowRunStatus::Failed
                | StatefulWorkflowRunStatus::DeadLettered
                | StatefulWorkflowRunStatus::Retrying
        )
    }
}

trait StatefulWaitStatusExt {
    fn is_active(&self) -> bool;
}

impl StatefulWaitStatusExt for StatefulWaitStatus {
    fn is_active(&self) -> bool {
        matches!(
            self,
            StatefulWaitStatus::Waiting
                | StatefulWaitStatus::Claimed
                | StatefulWaitStatus::Escalated
        )
    }
}
