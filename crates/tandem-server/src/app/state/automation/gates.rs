use std::collections::HashSet;

use serde_json::json;

use crate::{
    AutomationGateDecisionRecord, AutomationGateExpiryAction, AutomationGateExpiryPolicy,
    AutomationPendingGate, AutomationRunStatus, AutomationStopKind, AutomationV2RunRecord,
    AutomationV2Spec,
};

const DEFAULT_APPROVAL_GATE_EXPIRES_AFTER_MS: u64 = 7 * 24 * 60 * 60 * 1000;

pub(crate) enum AutomationGateDecisionOutcome {
    Applied,
    AlreadyDecided(Option<AutomationGateDecisionRecord>),
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
    if policy.on_expiry != Some(AutomationGateExpiryAction::Cancel) {
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
            _ => None,
        })
        .unwrap_or(AutomationGateExpiryAction::Cancel)
}

pub(crate) fn pause_automation_run_for_gate(
    run: &mut AutomationV2RunRecord,
    gate: AutomationPendingGate,
    blocked_nodes: Vec<String>,
) {
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
    if let Some(winner) = settled_gate_decision(run, &gate.node_id) {
        return AutomationGateDecisionOutcome::AlreadyDecided(Some(winner.clone()));
    }

    let gate_still_pending = run.status == AutomationRunStatus::AwaitingApproval
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
                    && !run
                        .checkpoint
                        .gate_history
                        .iter()
                        .any(|record| record.node_id == gate.node_id)
            });
    if !gate_still_pending {
        return AutomationGateDecisionOutcome::AlreadyDecided(
            run.checkpoint.gate_history.last().cloned(),
        );
    }

    run.checkpoint
        .gate_history
        .push(AutomationGateDecisionRecord {
            node_id: gate.node_id.clone(),
            decision: decision.to_string(),
            reason: reason.clone(),
            decided_at_ms: crate::now_ms(),
            decided_by,
            metadata: gate.metadata.clone(),
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
        "approve" => {
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
                    && !run
                        .checkpoint
                        .gate_history
                        .iter()
                        .any(|record| record.node_id == gate.node_id)
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
    if latest.decision == "rework" {
        None
    } else {
        Some(latest)
    }
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
