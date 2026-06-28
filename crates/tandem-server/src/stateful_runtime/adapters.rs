use serde_json::json;

use crate::automation_v2::types::{AutomationRunStatus, AutomationV2RunRecord};
use tandem_workflows::{WorkflowRunRecord, WorkflowRunStatus};

use super::types::{
    StatefulRuntimeScope, StatefulWaitKind, StatefulWorkflowRunKind, StatefulWorkflowRunRecord,
    StatefulWorkflowRunStatus, STATEFUL_RUNTIME_SCHEMA_VERSION,
};

pub fn stateful_run_from_automation_v2(run: &AutomationV2RunRecord) -> StatefulWorkflowRunRecord {
    let awaiting_gate = run.checkpoint.awaiting_gate.as_ref();
    StatefulWorkflowRunRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        run_id: run.run_id.clone(),
        kind: StatefulWorkflowRunKind::AutomationV2,
        workflow_id: None,
        automation_id: Some(run.automation_id.clone()),
        automation_run_id: Some(run.run_id.clone()),
        scope: StatefulRuntimeScope::from_tenant_context(run.tenant_context.clone()),
        status: automation_status_to_stateful(&run.status),
        trigger_type: Some(run.trigger_type.clone()),
        trigger_event: None,
        source_event_id: None,
        task_id: None,
        current_phase_id: awaiting_gate.map(|gate| gate.node_id.clone()),
        active_wait_kind: awaiting_gate.map(|_| StatefulWaitKind::Approval),
        active_wait_id: awaiting_gate.map(|gate| gate.node_id.clone()),
        workflow_definition_version: None,
        created_at_ms: run.created_at_ms,
        updated_at_ms: run.updated_at_ms,
        started_at_ms: run.started_at_ms,
        finished_at_ms: run.finished_at_ms,
        latest_snapshot_id: None,
        related_context_run_ids: run.active_instance_ids.clone(),
        metadata: Some(json!({
            "active_session_ids": &run.active_session_ids,
            "latest_session_id": &run.latest_session_id,
            "stop_kind": &run.stop_kind,
            "trigger_reason": &run.trigger_reason,
            "consumed_handoff_id": &run.consumed_handoff_id
        })),
    }
}

pub fn stateful_run_from_workflow(run: &WorkflowRunRecord) -> StatefulWorkflowRunRecord {
    let awaiting_gate = run.awaiting_gate.as_ref();
    StatefulWorkflowRunRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        run_id: run.run_id.clone(),
        kind: StatefulWorkflowRunKind::Workflow,
        workflow_id: Some(run.workflow_id.clone()),
        automation_id: run.automation_id.clone(),
        automation_run_id: run.automation_run_id.clone(),
        scope: StatefulRuntimeScope::from_tenant_context(run.tenant_context.clone()),
        status: workflow_status_to_stateful(&run.status),
        trigger_type: None,
        trigger_event: run.trigger_event.clone(),
        source_event_id: run.source_event_id.clone(),
        task_id: run.task_id.clone(),
        current_phase_id: awaiting_gate.map(|gate| gate.action_id.clone()),
        active_wait_kind: awaiting_gate.map(|_| StatefulWaitKind::Approval),
        active_wait_id: awaiting_gate.map(|gate| gate.action_id.clone()),
        workflow_definition_version: None,
        created_at_ms: run.created_at_ms,
        updated_at_ms: run.updated_at_ms,
        started_at_ms: None,
        finished_at_ms: run.finished_at_ms,
        latest_snapshot_id: None,
        related_context_run_ids: Vec::new(),
        metadata: run.binding_id.as_ref().map(|binding_id| {
            json!({
                "binding_id": binding_id,
                "source": &run.source
            })
        }),
    }
}

pub fn automation_status_to_stateful(status: &AutomationRunStatus) -> StatefulWorkflowRunStatus {
    match status {
        AutomationRunStatus::Queued => StatefulWorkflowRunStatus::Queued,
        AutomationRunStatus::Running => StatefulWorkflowRunStatus::Running,
        AutomationRunStatus::Pausing => StatefulWorkflowRunStatus::Pausing,
        AutomationRunStatus::Paused => StatefulWorkflowRunStatus::Paused,
        AutomationRunStatus::AwaitingApproval => StatefulWorkflowRunStatus::AwaitingApproval,
        AutomationRunStatus::Completed => StatefulWorkflowRunStatus::Completed,
        AutomationRunStatus::Blocked => StatefulWorkflowRunStatus::Blocked,
        AutomationRunStatus::Failed => StatefulWorkflowRunStatus::Failed,
        AutomationRunStatus::Cancelled => StatefulWorkflowRunStatus::Cancelled,
    }
}

pub fn workflow_status_to_stateful(status: &WorkflowRunStatus) -> StatefulWorkflowRunStatus {
    match status {
        WorkflowRunStatus::Queued => StatefulWorkflowRunStatus::Queued,
        WorkflowRunStatus::Running => StatefulWorkflowRunStatus::Running,
        WorkflowRunStatus::AwaitingApproval => StatefulWorkflowRunStatus::AwaitingApproval,
        WorkflowRunStatus::Completed => StatefulWorkflowRunStatus::Completed,
        WorkflowRunStatus::Failed => StatefulWorkflowRunStatus::Failed,
        WorkflowRunStatus::Cancelled => StatefulWorkflowRunStatus::Cancelled,
        WorkflowRunStatus::DryRun => StatefulWorkflowRunStatus::DryRun,
    }
}

#[cfg(test)]
mod tests {
    use crate::automation_v2::types::{
        AutomationRunCheckpoint, AutomationRunStatus, AutomationV2RunRecord,
    };
    use tandem_types::TenantContext;
    use tandem_workflows::{WorkflowRunRecord, WorkflowRunStatus};

    use super::*;

    fn tenant() -> TenantContext {
        TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a")
    }

    #[test]
    fn automation_adapter_preserves_tenant_and_wait_state() {
        let mut checkpoint = AutomationRunCheckpoint {
            completed_nodes: Vec::new(),
            pending_nodes: Vec::new(),
            node_outputs: Default::default(),
            node_attempts: Default::default(),
            node_attempt_verdicts: Default::default(),
            blocked_nodes: Vec::new(),
            awaiting_gate: None,
            gate_history: Vec::new(),
            lifecycle_history: Vec::new(),
            last_failure: None,
        };
        checkpoint.awaiting_gate = Some(crate::automation_v2::types::AutomationPendingGate {
            node_id: "approve-plan".to_string(),
            title: "Approve plan".to_string(),
            instructions: None,
            decisions: Vec::new(),
            rework_targets: Vec::new(),
            requested_at_ms: 123,
            upstream_node_ids: Vec::new(),
            metadata: None,
            expiry_policy: None,
        });

        let record = AutomationV2RunRecord {
            run_id: "run-a".to_string(),
            automation_id: "automation-a".to_string(),
            tenant_context: tenant(),
            trigger_type: "webhook".to_string(),
            status: AutomationRunStatus::AwaitingApproval,
            created_at_ms: 10,
            updated_at_ms: 20,
            started_at_ms: Some(11),
            finished_at_ms: None,
            active_session_ids: vec!["session-a".to_string()],
            latest_session_id: Some("session-a".to_string()),
            active_instance_ids: vec!["context-run-a".to_string()],
            checkpoint,
            runtime_context: None,
            automation_snapshot: None,
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
            scheduler: None,
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
            effective_execution_profile: Default::default(),
            requested_execution_profile: None,
        };

        let stateful = stateful_run_from_automation_v2(&record);

        assert_eq!(stateful.run_id, "run-a");
        assert_eq!(stateful.scope.organization_id(), "org-a");
        assert_eq!(stateful.status, StatefulWorkflowRunStatus::AwaitingApproval);
        assert_eq!(stateful.active_wait_kind, Some(StatefulWaitKind::Approval));
        assert_eq!(stateful.active_wait_id.as_deref(), Some("approve-plan"));
        assert_eq!(stateful.related_context_run_ids, vec!["context-run-a"]);
    }

    #[test]
    fn workflow_adapter_preserves_tenant_and_source_ids() {
        let record = WorkflowRunRecord {
            run_id: "workflow-run-a".to_string(),
            workflow_id: "workflow-a".to_string(),
            tenant_context: tenant(),
            automation_id: Some("automation-a".to_string()),
            automation_run_id: Some("automation-run-a".to_string()),
            binding_id: Some("binding-a".to_string()),
            trigger_event: Some("repo.pushed".to_string()),
            source_event_id: Some("event-a".to_string()),
            task_id: Some("task-a".to_string()),
            status: WorkflowRunStatus::Running,
            created_at_ms: 10,
            updated_at_ms: 20,
            finished_at_ms: None,
            actions: Vec::new(),
            awaiting_gate: None,
            gate_history: Vec::new(),
            source: None,
        };

        let stateful = stateful_run_from_workflow(&record);

        assert_eq!(stateful.kind, StatefulWorkflowRunKind::Workflow);
        assert_eq!(stateful.workflow_id.as_deref(), Some("workflow-a"));
        assert_eq!(
            stateful.automation_run_id.as_deref(),
            Some("automation-run-a")
        );
        assert_eq!(stateful.scope.workspace_id(), "workspace-a");
        assert_eq!(stateful.source_event_id.as_deref(), Some("event-a"));
        assert_eq!(stateful.status, StatefulWorkflowRunStatus::Running);
    }
}
