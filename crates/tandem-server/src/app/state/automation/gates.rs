use std::collections::HashSet;

use serde_json::json;

use crate::{
    AutomationGateDecisionRecord, AutomationPendingGate, AutomationRunStatus, AutomationStopKind,
    AutomationV2RunRecord, AutomationV2Spec,
};

pub(crate) enum AutomationGateDecisionOutcome {
    Applied,
    AlreadyDecided(Option<AutomationGateDecisionRecord>),
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

fn clear_automation_run_execution_handles(run: &mut AutomationV2RunRecord) {
    run.active_session_ids.clear();
    run.latest_session_id = None;
    run.active_instance_ids.clear();
}
