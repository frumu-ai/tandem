use std::collections::HashSet;

use serde_json::{json, Value};
use tandem_types::{ApprovalSourceKind, ApprovalWaitRef};

use crate::{
    AutomationGateDecisionRecord, AutomationGateExpiryAction, AutomationGateExpiryPolicy,
    AutomationPendingGate, AutomationRunStatus, AutomationStopKind, AutomationV2RunRecord,
    AutomationV2Spec,
};

const DEFAULT_APPROVAL_GATE_EXPIRES_AFTER_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const APPROVAL_WAIT_METADATA_KEY: &str = "approval_wait";
const TRANSITION_GUARD_METADATA_KEY: &str = "transition_guard";
const TRANSITION_GUARD_KIND: &str = "automation_v2_approval_transition";
const TRANSITION_GUARD_DENIED_DECISION: &str = "guard_denied";

#[derive(Debug)]
pub(crate) enum AutomationGateDecisionOutcome {
    Applied,
    AlreadyDecided(Option<AutomationGateDecisionRecord>),
}

#[derive(Debug, Clone)]
pub(crate) struct AutomationGateTransitionGuardDenial {
    pub code: &'static str,
    pub detail: String,
    pub metadata: Value,
}

pub(crate) fn effective_automation_gate_expiry_policy(
    gate: &AutomationPendingGate,
) -> Option<AutomationGateExpiryPolicy> {
    let mut policy = gate
        .expiry_policy
        .clone()
        .unwrap_or_else(default_approval_gate_expiry_policy);
    if policy.expires_after_ms == Some(0) {
        policy.expires_after_ms = None;
    }
    if policy.expires_after_ms.is_none() {
        return None;
    }
    if policy.on_expiry.is_none() {
        policy.on_expiry = Some(default_approval_gate_expiry_action());
    }
    if policy.remind_every_ms == Some(0) {
        policy.remind_every_ms = None;
    }
    Some(policy)
}

pub(crate) fn automation_gate_expires_at_ms(gate: &AutomationPendingGate) -> Option<u64> {
    let policy = effective_automation_gate_expiry_policy(gate)?;
    Some(
        gate.requested_at_ms
            .saturating_add(policy.expires_after_ms?),
    )
}

pub(crate) fn automation_gate_rejects_late_human_decision(
    gate: &AutomationPendingGate,
    now_ms: u64,
) -> bool {
    let Some(policy) = effective_automation_gate_expiry_policy(gate) else {
        return false;
    };
    if !matches!(
        policy.on_expiry,
        Some(AutomationGateExpiryAction::Cancel | AutomationGateExpiryAction::Resume)
    ) {
        return false;
    }
    let Some(expires_at_ms) = gate
        .requested_at_ms
        .checked_add(policy.expires_after_ms.unwrap_or_default())
    else {
        return false;
    };
    now_ms >= expires_at_ms
}

fn default_approval_gate_expiry_policy() -> AutomationGateExpiryPolicy {
    AutomationGateExpiryPolicy {
        expires_after_ms: default_approval_gate_expires_after_ms(),
        on_expiry: Some(default_approval_gate_expiry_action()),
        escalate_to: None,
        remind_every_ms: None,
    }
}

fn default_approval_gate_expires_after_ms() -> Option<u64> {
    std::env::var("TANDEM_APPROVAL_GATE_DEFAULT_EXPIRES_AFTER_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .or(Some(DEFAULT_APPROVAL_GATE_EXPIRES_AFTER_MS))
        .filter(|value| *value > 0)
}

fn default_approval_gate_expiry_action() -> AutomationGateExpiryAction {
    std::env::var("TANDEM_APPROVAL_GATE_DEFAULT_ON_EXPIRY")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .and_then(|value| match value.as_str() {
            "cancel" => Some(AutomationGateExpiryAction::Cancel),
            "escalate" => Some(AutomationGateExpiryAction::Escalate),
            "remind" => Some(AutomationGateExpiryAction::Remind),
            "resume" => Some(AutomationGateExpiryAction::Resume),
            _ => None,
        })
        .unwrap_or(AutomationGateExpiryAction::Cancel)
}

pub(crate) fn pause_automation_run_for_gate(
    run: &mut AutomationV2RunRecord,
    mut gate: AutomationPendingGate,
    blocked_nodes: Vec<String>,
) {
    bind_gate_transition_guard_metadata(run, &mut gate);
    run.status = AutomationRunStatus::AwaitingApproval;
    run.detail = Some(format!("awaiting approval for gate `{}`", gate.node_id));
    run.checkpoint.awaiting_gate = Some(gate);
    run.checkpoint.blocked_nodes = blocked_nodes;
}

pub(crate) fn apply_automation_gate_decision(
    run: &mut AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    gate: &AutomationPendingGate,
    decision: &str,
    reason: Option<String>,
    decided_by: Option<crate::automation_v2::governance::GovernanceActorRef>,
) -> AutomationGateDecisionOutcome {
    apply_automation_gate_decision_inner(run, automation, gate, decision, reason, decided_by, None)
}

pub(crate) fn automation_gate_has_settled_decision(
    run: &AutomationV2RunRecord,
    gate_node_id: &str,
) -> bool {
    settled_gate_decision(run, gate_node_id).is_some()
}

pub(crate) fn automation_gate_decision_settles_wait(decision: &str) -> bool {
    decision != "rework" && decision != TRANSITION_GUARD_DENIED_DECISION
}

pub(crate) fn apply_automation_gate_decision_with_transition_guard(
    run: &mut AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    gate: &AutomationPendingGate,
    decision: &str,
    reason: Option<String>,
    decided_by: Option<crate::automation_v2::governance::GovernanceActorRef>,
    requested_approval_request_id: Option<&str>,
    requested_transition_id: Option<&str>,
) -> Result<AutomationGateDecisionOutcome, AutomationGateTransitionGuardDenial> {
    if let Some(winner) = settled_gate_decision(run, &gate.node_id) {
        return Ok(AutomationGateDecisionOutcome::AlreadyDecided(Some(
            winner.clone(),
        )));
    }

    if !gate_is_still_pending(run, gate) {
        return Ok(AutomationGateDecisionOutcome::AlreadyDecided(
            run.checkpoint.gate_history.last().cloned(),
        ));
    }

    let approval_wait =
        ApprovalWaitRef::for_gate(ApprovalSourceKind::AutomationV2, &run.run_id, &gate.node_id);
    if let Err(denial) = validate_automation_gate_transition_guard(
        run,
        gate,
        &approval_wait,
        requested_approval_request_id,
        requested_transition_id,
    ) {
        record_transition_guard_denial(run, gate, &denial, decided_by);
        return Err(denial);
    }
    Ok(apply_automation_gate_decision_inner(
        run,
        automation,
        gate,
        decision,
        reason,
        decided_by,
        Some(approval_wait),
    ))
}

fn apply_automation_gate_decision_inner(
    run: &mut AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    gate: &AutomationPendingGate,
    decision: &str,
    reason: Option<String>,
    decided_by: Option<crate::automation_v2::governance::GovernanceActorRef>,
    approval_wait: Option<ApprovalWaitRef>,
) -> AutomationGateDecisionOutcome {
    if let Some(winner) = settled_gate_decision(run, &gate.node_id) {
        return AutomationGateDecisionOutcome::AlreadyDecided(Some(winner.clone()));
    }

    if !gate_is_still_pending(run, gate) {
        return AutomationGateDecisionOutcome::AlreadyDecided(
            run.checkpoint.gate_history.last().cloned(),
        );
    }

    let approval_wait = approval_wait.unwrap_or_else(|| {
        ApprovalWaitRef::for_gate(ApprovalSourceKind::AutomationV2, &run.run_id, &gate.node_id)
    });
    run.checkpoint
        .gate_history
        .push(AutomationGateDecisionRecord {
            node_id: gate.node_id.clone(),
            decision: decision.to_string(),
            reason: reason.clone(),
            decided_at_ms: crate::now_ms(),
            decided_by,
            metadata: gate_decision_metadata_with_approval_wait(
                gate.metadata.clone(),
                &approval_wait,
                Some(run),
            ),
        });
    run.checkpoint.awaiting_gate = None;
    match decision {
        "approve" => apply_gate_approval(run, gate, reason),
        "rework" => apply_gate_rework(run, automation, gate),
        "cancel" => apply_gate_cancel(run, gate, reason),
        _ => {}
    }
    if decision != "cancel" {
        run.resume_reason = Some(format!("gate `{}` decision: {}", gate.node_id, decision));
        clear_automation_run_execution_handles(run);
        crate::refresh_automation_runtime_state(automation, run);
    }
    AutomationGateDecisionOutcome::Applied
}

pub(crate) fn apply_automation_gate_expiry(
    run: &mut AutomationV2RunRecord,
    gate: &AutomationPendingGate,
    reason: Option<String>,
    expires_at_ms: u64,
    policy: &AutomationGateExpiryPolicy,
) -> AutomationGateDecisionOutcome {
    if let Some(winner) = settled_gate_decision(run, &gate.node_id) {
        return AutomationGateDecisionOutcome::AlreadyDecided(Some(winner.clone()));
    }

    if !gate_is_still_pending(run, gate) {
        return AutomationGateDecisionOutcome::AlreadyDecided(
            run.checkpoint.gate_history.last().cloned(),
        );
    }

    let expired_at_ms = crate::now_ms();
    let approval_wait =
        ApprovalWaitRef::for_gate(ApprovalSourceKind::AutomationV2, &run.run_id, &gate.node_id);
    run.checkpoint
        .gate_history
        .push(AutomationGateDecisionRecord {
            node_id: gate.node_id.clone(),
            decision: "expired".to_string(),
            reason: reason.clone(),
            decided_at_ms: expired_at_ms,
            decided_by: Some(
                crate::automation_v2::governance::GovernanceActorRef::system(
                    "automation_gate_expiry",
                ),
            ),
            metadata: Some(json!({
                "approval_wait": approval_wait.clone(),
                "transition_guard": transition_guard_metadata(&approval_wait, Some(run)),
                "expiry_policy": policy,
                "expires_at_ms": expires_at_ms,
                "expired_at_ms": expired_at_ms,
                "gate_metadata": gate.metadata.clone(),
            })),
        });
    run.checkpoint.awaiting_gate = None;
    apply_gate_expired(run, gate, reason);
    AutomationGateDecisionOutcome::Applied
}

pub(crate) fn apply_automation_gate_timeout_resume(
    run: &mut AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    gate: &AutomationPendingGate,
    reason: Option<String>,
    expires_at_ms: u64,
    policy: &AutomationGateExpiryPolicy,
) -> AutomationGateDecisionOutcome {
    if let Some(winner) = settled_gate_decision(run, &gate.node_id) {
        return AutomationGateDecisionOutcome::AlreadyDecided(Some(winner.clone()));
    }
    if !gate_is_still_pending(run, gate) {
        return AutomationGateDecisionOutcome::AlreadyDecided(
            run.checkpoint.gate_history.last().cloned(),
        );
    }

    let resumed_at_ms = crate::now_ms();
    let approval_wait =
        ApprovalWaitRef::for_gate(ApprovalSourceKind::AutomationV2, &run.run_id, &gate.node_id);
    run.checkpoint
        .gate_history
        .push(AutomationGateDecisionRecord {
            node_id: gate.node_id.clone(),
            decision: "timeout_resume".to_string(),
            reason: reason.clone(),
            decided_at_ms: resumed_at_ms,
            decided_by: Some(
                crate::automation_v2::governance::GovernanceActorRef::system(
                    "automation_gate_timeout_resume",
                ),
            ),
            metadata: Some(json!({
                "approval_wait": approval_wait.clone(),
                "transition_guard": transition_guard_metadata(&approval_wait, Some(run)),
                "expiry_policy": policy,
                "expires_at_ms": expires_at_ms,
                "resumed_at_ms": resumed_at_ms,
                "gate_metadata": gate.metadata.clone(),
            })),
        });
    run.checkpoint.awaiting_gate = None;
    apply_gate_approval(run, gate, reason.clone());
    if let Some(output) = run.checkpoint.node_outputs.get_mut(&gate.node_id) {
        output["summary"] = json!(format!(
            "Gate `{}` resumed automatically after timeout.",
            gate.node_id
        ));
        output["content"]["decision"] = json!("timeout_resume");
    }
    run.resume_reason = Some(format!(
        "approval wait `{}` resumed automatically after timeout",
        gate.node_id
    ));
    clear_automation_run_execution_handles(run);
    crate::refresh_automation_runtime_state(automation, run);
    AutomationGateDecisionOutcome::Applied
}

pub(crate) fn recover_settled_automation_gate_decision(
    run: &mut AutomationV2RunRecord,
    automation: &AutomationV2Spec,
) -> bool {
    if run.status != AutomationRunStatus::AwaitingApproval {
        return false;
    }
    let Some(gate) = run.checkpoint.awaiting_gate.clone() else {
        return false;
    };
    let Some(record) = settled_gate_decision(run, &gate.node_id).cloned() else {
        return false;
    };

    run.checkpoint.awaiting_gate = None;
    match record.decision.as_str() {
        "approve" | "timeout_resume" => {
            apply_gate_approval(run, &gate, record.reason.clone());
            run.resume_reason = Some(format!(
                "recovered settled gate `{}` approval after restart",
                gate.node_id
            ));
            clear_automation_run_execution_handles(run);
            crate::record_automation_lifecycle_event(
                run,
                "approval_gate_decision_recovered",
                Some(format!(
                    "recovered approved gate `{}` after restart",
                    gate.node_id
                )),
                None,
            );
            crate::refresh_automation_runtime_state(automation, run);
            true
        }
        "cancel" => {
            apply_gate_cancel(run, &gate, record.reason.clone());
            crate::record_automation_lifecycle_event(
                run,
                "approval_gate_decision_recovered",
                Some(format!(
                    "recovered cancelled gate `{}` after restart",
                    gate.node_id
                )),
                Some(AutomationStopKind::Cancelled),
            );
            true
        }
        "expired" => {
            apply_gate_expired(run, &gate, record.reason.clone());
            crate::record_automation_lifecycle_event(
                run,
                "approval_gate_decision_recovered",
                Some(format!(
                    "recovered expired gate `{}` after restart",
                    gate.node_id
                )),
                Some(AutomationStopKind::Cancelled),
            );
            true
        }
        _ => false,
    }
}

fn gate_decision_metadata_with_approval_wait(
    metadata: Option<Value>,
    approval_wait: &ApprovalWaitRef,
    run: Option<&AutomationV2RunRecord>,
) -> Option<Value> {
    let transition_guard = transition_guard_metadata(approval_wait, run);
    match metadata {
        Some(Value::Object(mut object)) => {
            object.insert(APPROVAL_WAIT_METADATA_KEY.to_string(), json!(approval_wait));
            object.insert(TRANSITION_GUARD_METADATA_KEY.to_string(), transition_guard);
            Some(Value::Object(object))
        }
        Some(gate_metadata) => Some(json!({
            "approval_wait": approval_wait,
            "transition_guard": transition_guard,
            "gate_metadata": gate_metadata,
        })),
        None => Some(json!({
            "approval_wait": approval_wait,
            "transition_guard": transition_guard,
        })),
    }
}

fn gate_is_still_pending(run: &AutomationV2RunRecord, gate: &AutomationPendingGate) -> bool {
    run.status == AutomationRunStatus::AwaitingApproval
        && run
            .checkpoint
            .awaiting_gate
            .as_ref()
            .map(|pending| pending.node_id == gate.node_id)
            .unwrap_or_else(|| {
                run.checkpoint
                    .pending_nodes
                    .iter()
                    .any(|node_id| node_id == &gate.node_id)
                    && settled_gate_decision(run, &gate.node_id).is_none()
            })
}

fn settled_gate_decision<'a>(
    run: &'a AutomationV2RunRecord,
    gate_node_id: &str,
) -> Option<&'a AutomationGateDecisionRecord> {
    let latest = run
        .checkpoint
        .gate_history
        .iter()
        .rev()
        .find(|record| record.node_id == gate_node_id)?;
    if automation_gate_decision_settles_wait(&latest.decision) {
        Some(latest)
    } else {
        None
    }
}

fn bind_gate_transition_guard_metadata(
    run: &AutomationV2RunRecord,
    gate: &mut AutomationPendingGate,
) {
    let approval_wait =
        ApprovalWaitRef::for_gate(ApprovalSourceKind::AutomationV2, &run.run_id, &gate.node_id);
    gate.metadata =
        gate_decision_metadata_with_approval_wait(gate.metadata.take(), &approval_wait, Some(run));
}

fn transition_guard_metadata(
    approval_wait: &ApprovalWaitRef,
    run: Option<&AutomationV2RunRecord>,
) -> Value {
    let tenant = run.map(|run| {
        json!({
            "org_id": run.tenant_context.org_id.clone(),
            "workspace_id": run.tenant_context.workspace_id.clone(),
            "actor_id": run.tenant_context.actor_id.clone(),
        })
    });
    json!({
        "kind": TRANSITION_GUARD_KIND,
        "source": approval_wait.source.as_str(),
        "approval_request_id": approval_wait.approval_request_id.clone(),
        "wait_id": approval_wait.wait_id.clone(),
        "transition_id": approval_wait.transition_id.clone(),
        "run_id": approval_wait.run_id.clone(),
        "node_id": approval_wait.node_id.clone(),
        "tenant": tenant,
    })
}

fn validate_automation_gate_transition_guard(
    run: &AutomationV2RunRecord,
    gate: &AutomationPendingGate,
    expected: &ApprovalWaitRef,
    requested_approval_request_id: Option<&str>,
    requested_transition_id: Option<&str>,
) -> Result<(), AutomationGateTransitionGuardDenial> {
    if let Some(actual) = normalized_guard_value(requested_approval_request_id) {
        if actual != expected.approval_request_id {
            return Err(transition_guard_denial(
                run,
                gate,
                expected,
                "approval_request_id_mismatch",
                format!(
                    "approval request `{actual}` cannot unlock gate `{}` for run `{}`",
                    gate.node_id, run.run_id
                ),
                Some(json!({ "approval_request_id": actual })),
            ));
        }
    }
    if let Some(actual) = normalized_guard_value(requested_transition_id) {
        if Some(actual.as_str()) != expected.transition_id.as_deref() {
            return Err(transition_guard_denial(
                run,
                gate,
                expected,
                "transition_id_mismatch",
                format!(
                    "transition `{actual}` cannot unlock gate `{}` for run `{}`",
                    gate.node_id, run.run_id
                ),
                Some(json!({ "transition_id": actual })),
            ));
        }
    }
    if let Some(pending) = run
        .checkpoint
        .awaiting_gate
        .as_ref()
        .filter(|pending| pending.node_id == gate.node_id)
    {
        validate_gate_metadata_transition_guard(
            run,
            gate,
            expected,
            "pending_gate",
            pending.metadata.as_ref(),
        )?;
    }
    validate_gate_metadata_transition_guard(
        run,
        gate,
        expected,
        "submitted_gate",
        gate.metadata.as_ref(),
    )
}

fn validate_gate_metadata_transition_guard(
    run: &AutomationV2RunRecord,
    gate: &AutomationPendingGate,
    expected: &ApprovalWaitRef,
    metadata_scope: &str,
    metadata: Option<&Value>,
) -> Result<(), AutomationGateTransitionGuardDenial> {
    let Some(metadata) = metadata else {
        return Ok(());
    };
    if let Some(wait_value) = metadata.get(APPROVAL_WAIT_METADATA_KEY) {
        let wait = serde_json::from_value::<ApprovalWaitRef>(wait_value.clone()).map_err(|_| {
            transition_guard_denial(
                run,
                gate,
                expected,
                "approval_wait_invalid",
                format!("approval wait metadata on {metadata_scope} is invalid"),
                Some(json!({ "metadata_scope": metadata_scope })),
            )
        })?;
        if wait.approval_request_id != expected.approval_request_id
            || wait.run_id != expected.run_id
            || wait.node_id.as_deref() != expected.node_id.as_deref()
            || wait.transition_id.as_deref() != expected.transition_id.as_deref()
            || wait.source != expected.source
        {
            return Err(transition_guard_denial(
                run,
                gate,
                expected,
                "approval_wait_mismatch",
                format!("approval wait metadata on {metadata_scope} belongs to another transition"),
                Some(json!({
                    "metadata_scope": metadata_scope,
                    "approval_wait": wait,
                })),
            ));
        }
    }
    let Some(guard) = metadata
        .get(TRANSITION_GUARD_METADATA_KEY)
        .and_then(Value::as_object)
    else {
        return Ok(());
    };
    let source = guard.get("source").and_then(Value::as_str);
    let approval_request_id = guard.get("approval_request_id").and_then(Value::as_str);
    let wait_id = guard.get("wait_id").and_then(Value::as_str);
    let transition_id = guard.get("transition_id").and_then(Value::as_str);
    let run_id = guard.get("run_id").and_then(Value::as_str);
    let node_id = guard.get("node_id").and_then(Value::as_str);
    let kind = guard.get("kind").and_then(Value::as_str);
    let matches_expected = kind == Some(TRANSITION_GUARD_KIND)
        && source == Some(expected.source.as_str())
        && approval_request_id == Some(expected.approval_request_id.as_str())
        && wait_id == Some(expected.wait_id.as_str())
        && transition_id == expected.transition_id.as_deref()
        && run_id == Some(expected.run_id.as_str())
        && node_id == expected.node_id.as_deref()
        && guard_tenant_matches(run, guard.get("tenant"));
    if matches_expected {
        return Ok(());
    }
    Err(transition_guard_denial(
        run,
        gate,
        expected,
        "transition_guard_metadata_mismatch",
        format!("transition guard metadata on {metadata_scope} belongs to another transition"),
        Some(json!({
            "metadata_scope": metadata_scope,
            "transition_guard": guard,
        })),
    ))
}

fn guard_tenant_matches(run: &AutomationV2RunRecord, tenant: Option<&Value>) -> bool {
    let Some(tenant) = tenant else {
        return true;
    };
    if tenant.is_null() {
        return true;
    }
    tenant.get("org_id").and_then(Value::as_str) == Some(run.tenant_context.org_id.as_str())
        && tenant.get("workspace_id").and_then(Value::as_str)
            == Some(run.tenant_context.workspace_id.as_str())
        && tenant.get("actor_id").and_then(Value::as_str) == run.tenant_context.actor_id.as_deref()
}

fn normalized_guard_value(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn transition_guard_denial(
    run: &AutomationV2RunRecord,
    gate: &AutomationPendingGate,
    expected: &ApprovalWaitRef,
    reason_code: &'static str,
    detail: String,
    evidence: Option<Value>,
) -> AutomationGateTransitionGuardDenial {
    AutomationGateTransitionGuardDenial {
        code: "AUTOMATION_V2_GATE_TRANSITION_GUARD_DENIED",
        detail,
        metadata: json!({
            "reason_code": reason_code,
            "run_id": run.run_id.clone(),
            "automation_id": run.automation_id.clone(),
            "node_id": gate.node_id.clone(),
            "expected_approval_request_id": expected.approval_request_id.clone(),
            "expected_transition_id": expected.transition_id.clone(),
            "tenant": {
                "org_id": run.tenant_context.org_id.clone(),
                "workspace_id": run.tenant_context.workspace_id.clone(),
                "actor_id": run.tenant_context.actor_id.clone(),
            },
            "evidence": evidence,
        }),
    }
}

fn record_transition_guard_denial(
    run: &mut AutomationV2RunRecord,
    gate: &AutomationPendingGate,
    denial: &AutomationGateTransitionGuardDenial,
    decided_by: Option<crate::automation_v2::governance::GovernanceActorRef>,
) {
    run.detail = Some(format!(
        "approval transition guard denied for gate `{}`: {}",
        gate.node_id, denial.detail
    ));
    run.checkpoint
        .gate_history
        .push(AutomationGateDecisionRecord {
            node_id: gate.node_id.clone(),
            decision: TRANSITION_GUARD_DENIED_DECISION.to_string(),
            reason: Some(denial.detail.clone()),
            decided_at_ms: crate::now_ms(),
            decided_by,
            metadata: Some(json!({
                "transition_guard_denial": denial.metadata.clone(),
            })),
        });
    crate::record_automation_lifecycle_event_with_metadata(
        run,
        "approval_gate_transition_guard_denied",
        Some(denial.detail.clone()),
        None,
        Some(denial.metadata.clone()),
    );
}

fn apply_gate_approval(
    run: &mut AutomationV2RunRecord,
    gate: &AutomationPendingGate,
    reason: Option<String>,
) {
    run.status = AutomationRunStatus::Queued;
    run.detail = Some(format!("gate `{}` approved", gate.node_id));
    run.stop_kind = None;
    run.stop_reason = None;
    run.checkpoint
        .blocked_nodes
        .retain(|node_id| node_id != &gate.node_id);
    if run
        .checkpoint
        .last_failure
        .as_ref()
        .is_some_and(|failure| failure.node_id == gate.node_id)
    {
        run.checkpoint.last_failure = None;
    }
    run.checkpoint
        .pending_nodes
        .retain(|node_id| node_id != &gate.node_id);
    if !run
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == &gate.node_id)
    {
        run.checkpoint.completed_nodes.push(gate.node_id.clone());
    }
    run.checkpoint.node_outputs.insert(
        gate.node_id.clone(),
        json!({
            "contract_kind": "approval_gate",
            // Terminal-run accounting (`derive_terminal_run_state`) requires a
            // terminal `status`; without it an approved gate node derives as
            // "incomplete" and fails the run at finalization.
            "status": "completed",
            "summary": format!("Gate `{}` approved.", gate.node_id),
            "content": {
                "decision": "approve",
                "reason": reason,
            },
            "created_at_ms": crate::now_ms(),
            "node_id": gate.node_id.clone(),
        }),
    );
}

fn apply_gate_rework(
    run: &mut AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    gate: &AutomationPendingGate,
) {
    run.status = AutomationRunStatus::Queued;
    run.detail = Some(format!("gate `{}` sent work back for rework", gate.node_id));
    run.stop_kind = None;
    run.stop_reason = None;
    let mut roots = gate.rework_targets.iter().cloned().collect::<HashSet<_>>();
    if roots.is_empty() {
        roots.extend(gate.upstream_node_ids.iter().cloned());
    }
    roots.insert(gate.node_id.clone());
    let reset_nodes = crate::app::state::collect_automation_descendants(automation, &roots);
    for node_id in &reset_nodes {
        run.checkpoint.node_outputs.remove(node_id);
        run.checkpoint.node_attempts.remove(node_id);
    }
    run.checkpoint
        .completed_nodes
        .retain(|node_id| !reset_nodes.contains(node_id));
    let mut pending = run.checkpoint.pending_nodes.clone();
    for node_id in reset_nodes {
        if !pending.iter().any(|existing| existing == &node_id) {
            pending.push(node_id);
        }
    }
    pending.sort();
    pending.dedup();
    run.checkpoint.pending_nodes = pending;
}

fn apply_gate_cancel(
    run: &mut AutomationV2RunRecord,
    gate: &AutomationPendingGate,
    reason: Option<String>,
) {
    run.status = AutomationRunStatus::Cancelled;
    let stop_reason = reason
        .clone()
        .unwrap_or_else(|| format!("gate `{}` cancelled the run", gate.node_id));
    run.detail = Some(stop_reason.clone());
    run.stop_kind = Some(AutomationStopKind::Cancelled);
    run.stop_reason = Some(stop_reason.clone());
    crate::record_automation_lifecycle_event(
        run,
        "run_cancelled",
        Some(stop_reason),
        Some(AutomationStopKind::Cancelled),
    );
}

fn apply_gate_expired(
    run: &mut AutomationV2RunRecord,
    gate: &AutomationPendingGate,
    reason: Option<String>,
) {
    run.status = AutomationRunStatus::Cancelled;
    let stop_reason = reason
        .clone()
        .unwrap_or_else(|| format!("gate `{}` expired before approval", gate.node_id));
    run.detail = Some(stop_reason.clone());
    run.stop_kind = Some(AutomationStopKind::Cancelled);
    run.stop_reason = Some(stop_reason.clone());
    crate::record_automation_lifecycle_event(
        run,
        "approval_gate_expired",
        Some(stop_reason.clone()),
        Some(AutomationStopKind::Cancelled),
    );
    crate::record_automation_lifecycle_event(
        run,
        "run_cancelled",
        Some(stop_reason),
        Some(AutomationStopKind::Cancelled),
    );
}

fn clear_automation_run_execution_handles(run: &mut AutomationV2RunRecord) {
    run.active_session_ids.clear();
    run.latest_session_id = None;
    run.active_instance_ids.clear();
}
