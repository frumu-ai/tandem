//! Cross-subsystem aggregator for pending approvals.
//!
//! Surfaces a unified list of [`ApprovalRequest`]s drawn from every Tandem
//! subsystem that owns a pending-approval primitive.
//!
//! v1 sources: `automation_v2` mission runs whose `checkpoint.awaiting_gate`
//! is set or can be recovered from a pending approval node. Workflow runs and
//! coder runs will be added once their pause/resume paths are wired (see
//! `docs/internal/approval-gates-and-channel-ux/PLAN.md`).
//!
//! The aggregator never mutates state. Decisions still go through the
//! authoritative subsystem handlers (e.g. `automations_v2_run_gate_decide`);
//! a unified `/approvals/{id}/decide` endpoint is intentionally deferred until
//! at least two source subsystems are wired.

use tandem_types::{
    ApprovalDecision, ApprovalListFilter, ApprovalRequest, ApprovalSourceKind, ApprovalTenantRef,
};

use crate::automation_v2::types::{
    AutomationPendingGate, AutomationRunStatus, AutomationV2RunRecord, AutomationV2Spec,
};
use crate::AppState;

/// Default cap on returned approvals when no `limit` is supplied.
const DEFAULT_PENDING_LIMIT: usize = 100;
/// Hard upper bound regardless of caller-supplied `limit`.
const MAX_PENDING_LIMIT: usize = 500;

/// Aggregate every pending approval matching `filter`.
///
/// Today this walks automation-v2 run history, including sharded run records.
/// The list is ordered most-recent first by `requested_at_ms`. Surfaces are expected to apply additional
/// per-user filtering (e.g. only show approvals targeting the current user)
/// at the surface layer; this aggregator does tenant filtering only.
pub async fn list_pending_approvals(
    state: &AppState,
    filter: &ApprovalListFilter,
) -> Vec<ApprovalRequest> {
    let limit = filter
        .limit
        .map(|value| (value as usize).min(MAX_PENDING_LIMIT))
        .unwrap_or(DEFAULT_PENDING_LIMIT);

    let mut out: Vec<ApprovalRequest> = Vec::new();

    if filter
        .source
        .as_ref()
        .map(|source| matches!(source, ApprovalSourceKind::AutomationV2))
        .unwrap_or(true)
    {
        let runs = state.list_automation_v2_runs(None, MAX_PENDING_LIMIT).await;
        for run in runs.iter() {
            if run.status != AutomationRunStatus::AwaitingApproval {
                continue;
            }
            let gate = run.checkpoint.awaiting_gate.clone().or_else(|| {
                run.automation_snapshot
                    .as_ref()
                    .and_then(|automation| recover_automation_v2_pending_gate(run, automation))
            });
            let Some(gate) = gate else {
                continue;
            };
            if !tenant_matches(filter, run) {
                continue;
            }
            out.push(automation_v2_run_to_approval_request(run, &gate));
        }
    }

    // Future: coder + workflow sources slot in here.

    out.sort_by(|a, b| b.requested_at_ms.cmp(&a.requested_at_ms));
    out.truncate(limit);
    out
}

fn recover_automation_v2_pending_gate(
    run: &AutomationV2RunRecord,
    automation: &AutomationV2Spec,
) -> Option<AutomationPendingGate> {
    let pending_nodes = run
        .checkpoint
        .pending_nodes
        .iter()
        .collect::<std::collections::HashSet<_>>();
    automation
        .flow
        .nodes
        .iter()
        .find(|node| {
            pending_nodes.contains(&node.node_id)
                && !run
                    .checkpoint
                    .gate_history
                    .iter()
                    .any(|record| record.node_id == node.node_id)
                && crate::app::state::is_automation_approval_node(node)
        })
        .and_then(crate::app::state::build_automation_pending_gate)
        .map(|mut gate| {
            gate.requested_at_ms = run.updated_at_ms.max(run.created_at_ms);
            gate
        })
}

fn tenant_matches(filter: &ApprovalListFilter, run: &AutomationV2RunRecord) -> bool {
    if let Some(org) = filter.org_id.as_deref() {
        if run.tenant_context.org_id != org {
            return false;
        }
    }
    if let Some(workspace) = filter.workspace_id.as_deref() {
        if run.tenant_context.workspace_id != workspace {
            return false;
        }
    }
    true
}

pub(crate) fn automation_v2_run_to_approval_request(
    run: &AutomationV2RunRecord,
    gate: &AutomationPendingGate,
) -> ApprovalRequest {
    let workflow_name = run
        .automation_snapshot
        .as_ref()
        .map(|snap| snap.name.clone())
        .or_else(|| Some(run.automation_id.clone()));

    let action_kind = run.automation_snapshot.as_ref().and_then(|snap| {
        snap.flow
            .nodes
            .iter()
            .find(|node| node.node_id == gate.node_id)
            .map(|node| node.objective.clone())
    });

    ApprovalRequest {
        request_id: format!("automation_v2:{}:{}", run.run_id, gate.node_id),
        source: ApprovalSourceKind::AutomationV2,
        tenant: ApprovalTenantRef {
            org_id: run.tenant_context.org_id.clone(),
            workspace_id: run.tenant_context.workspace_id.clone(),
            user_id: run.tenant_context.actor_id.clone(),
        },
        run_id: run.run_id.clone(),
        node_id: Some(gate.node_id.clone()),
        workflow_name,
        action_kind,
        action_preview_markdown: gate.instructions.clone(),
        surface_payload: Some(serde_json::json!({
            "automation_v2_run_id": run.run_id,
            "automation_id": run.automation_id,
            "node_id": gate.node_id,
            "decide_endpoint": format!(
                "/automations/v2/runs/{}/gate",
                run.run_id
            ),
        })),
        requested_at_ms: gate.requested_at_ms,
        expires_at_ms: None,
        decisions: gate
            .decisions
            .iter()
            .filter_map(|raw| match raw.to_ascii_lowercase().as_str() {
                "approve" => Some(ApprovalDecision::Approve),
                "rework" => Some(ApprovalDecision::Rework),
                "cancel" => Some(ApprovalDecision::Cancel),
                _ => None,
            })
            .collect(),
        rework_targets: gate.rework_targets.clone(),
        instructions: gate.instructions.clone(),
        decided_by: None,
        decided_at_ms: None,
        decision: None,
        rework_feedback: None,
    }
}
