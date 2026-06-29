use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use tandem_types::{DataClass, PrincipalRef, ResourceScope, ToolRiskTier};

use crate::automation_v2::types::{AutomationRunStatus, AutomationV2RunRecord};
use tandem_workflows::{WorkflowRunRecord, WorkflowRunStatus};

use super::definition::{
    automation_definition_snapshot_hash, automation_definition_version,
    automation_run_definition_fields,
};
use super::phases::phase_state_from_status;
use super::types::{
    StatefulRuntimeScope, StatefulWaitKind, StatefulWorkflowRunKind, StatefulWorkflowRunRecord,
    StatefulWorkflowRunStatus, STATEFUL_RUNTIME_SCHEMA_VERSION,
};

pub fn stateful_run_from_automation_v2(run: &AutomationV2RunRecord) -> StatefulWorkflowRunRecord {
    let awaiting_gate = run.checkpoint.awaiting_gate.as_ref();
    let current_phase_id = awaiting_gate.map(|gate| gate.node_id.clone());
    let status = automation_status_to_stateful(&run.status);
    let phase_state = phase_state_from_status(
        &run.run_id,
        &status,
        run.updated_at_ms,
        current_phase_id.as_deref(),
    );
    let (workflow_definition_version, workflow_definition_snapshot_hash) =
        automation_run_definition_fields(run);
    StatefulWorkflowRunRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        run_id: run.run_id.clone(),
        kind: StatefulWorkflowRunKind::AutomationV2,
        workflow_id: None,
        automation_id: Some(run.automation_id.clone()),
        automation_run_id: Some(run.run_id.clone()),
        scope: stateful_scope_from_automation_v2(run),
        status,
        phase: phase_state.phase,
        phase_history: phase_state.phase_history,
        allowed_next_phases: phase_state.allowed_next_phases,
        trigger_type: Some(run.trigger_type.clone()),
        trigger_event: None,
        source_event_id: None,
        task_id: None,
        current_phase_id,
        active_wait_kind: awaiting_gate.map(|_| StatefulWaitKind::Approval),
        active_wait_id: awaiting_gate.map(|gate| gate.node_id.clone()),
        workflow_definition_version,
        workflow_definition_snapshot_hash,
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

fn stateful_scope_from_automation_v2(run: &AutomationV2RunRecord) -> StatefulRuntimeScope {
    let mut scope = StatefulRuntimeScope::from_tenant_context(run.tenant_context.clone());
    if let Some(metadata) = automation_webhook_metadata(run) {
        merge_enterprise_scope_metadata(&mut scope, metadata);
    }
    scope
}

fn automation_webhook_metadata(run: &AutomationV2RunRecord) -> Option<&Value> {
    run.automation_snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.metadata.as_ref())
        .and_then(|metadata| metadata.get("automation_webhook"))
}

fn merge_enterprise_scope_metadata(scope: &mut StatefulRuntimeScope, metadata: &Value) {
    if scope.owner_principal.is_none() {
        scope.owner_principal = json_field(metadata, "owner_principal");
    }
    if scope.owning_org_unit_id.is_none() {
        scope.owning_org_unit_id = metadata
            .get("owning_org_unit_id")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
    }
    if scope.resource_scope.is_none() {
        scope.resource_scope = json_field::<ResourceScope>(metadata, "resource_scope");
    }
    if scope.data_classes.is_empty() {
        if let Some(data_classes) = json_field::<Vec<DataClass>>(metadata, "data_classes") {
            scope.data_classes = data_classes;
        } else if let Some(data_class) = json_field::<DataClass>(metadata, "data_class") {
            scope.data_classes = vec![data_class];
        }
    }
    if scope.risk_tier.is_none() {
        scope.risk_tier = json_field::<ToolRiskTier>(metadata, "risk_tier");
    }
    if scope.policy_version_id.is_none() {
        scope.policy_version_id = metadata
            .get("policy_version_id")
            .or_else(|| metadata.get("policy_version"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
    }
    if scope.delegation_grant_ids.is_empty() {
        scope.delegation_grant_ids =
            json_field::<Vec<String>>(metadata, "delegation_grant_ids").unwrap_or_default();
    }
}

fn json_field<T: DeserializeOwned>(metadata: &Value, key: &str) -> Option<T> {
    metadata
        .get(key)
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

pub fn stateful_run_from_workflow(run: &WorkflowRunRecord) -> StatefulWorkflowRunRecord {
    let awaiting_gate = run.awaiting_gate.as_ref();
    let current_phase_id = awaiting_gate.map(|gate| gate.action_id.clone());
    let status = workflow_status_to_stateful(&run.status);
    let phase_state = phase_state_from_status(
        &run.run_id,
        &status,
        run.updated_at_ms,
        current_phase_id.as_deref(),
    );
    StatefulWorkflowRunRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        run_id: run.run_id.clone(),
        kind: StatefulWorkflowRunKind::Workflow,
        workflow_id: Some(run.workflow_id.clone()),
        automation_id: run.automation_id.clone(),
        automation_run_id: run.automation_run_id.clone(),
        scope: StatefulRuntimeScope::from_tenant_context(run.tenant_context.clone()),
        status,
        phase: phase_state.phase,
        phase_history: phase_state.phase_history,
        allowed_next_phases: phase_state.allowed_next_phases,
        trigger_type: None,
        trigger_event: run.trigger_event.clone(),
        source_event_id: run.source_event_id.clone(),
        task_id: run.task_id.clone(),
        current_phase_id,
        active_wait_kind: awaiting_gate.map(|_| StatefulWaitKind::Approval),
        active_wait_id: awaiting_gate.map(|gate| gate.action_id.clone()),
        workflow_definition_version: None,
        workflow_definition_snapshot_hash: None,
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
        AutomationExecutionPolicy, AutomationFlowSpec, AutomationRunCheckpoint,
        AutomationRunStatus, AutomationV2RunRecord, AutomationV2Schedule, AutomationV2ScheduleType,
        AutomationV2Spec, AutomationV2Status,
    };
    use crate::stateful_runtime::StatefulWorkflowPhase;
    use serde_json::json;
    use tandem_types::{
        DataClass, PrincipalKind, PrincipalRef, ResourceKind, ResourceRef, ResourceScope,
        TenantContext, ToolRiskTier,
    };
    use tandem_workflows::{WorkflowRunRecord, WorkflowRunStatus};

    use super::*;

    fn tenant() -> TenantContext {
        TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a")
    }

    fn automation_snapshot(automation_id: &str) -> AutomationV2Spec {
        AutomationV2Spec {
            automation_id: automation_id.to_string(),
            name: "Definition test".to_string(),
            description: None,
            status: AutomationV2Status::Active,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
            },
            knowledge: Default::default(),
            agents: Vec::new(),
            flow: AutomationFlowSpec { nodes: Vec::new() },
            execution: AutomationExecutionPolicy::default(),
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 2,
            creator_id: "user-a".to_string(),
            workspace_root: None,
            metadata: Some(json!({ "definition_version": "release-17" })),
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        }
    }

    fn automation_run(
        automation_snapshot: Option<AutomationV2Spec>,
        checkpoint: AutomationRunCheckpoint,
    ) -> AutomationV2RunRecord {
        AutomationV2RunRecord {
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
            automation_snapshot,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            execution_claim: None,
            execution_claim_epoch: 0,
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
        }
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

        let snapshot = automation_snapshot("automation-a");
        let expected_hash = automation_definition_snapshot_hash(&snapshot);
        let record = automation_run(Some(snapshot), checkpoint);

        let stateful = stateful_run_from_automation_v2(&record);

        assert_eq!(stateful.run_id, "run-a");
        assert_eq!(stateful.scope.organization_id(), "org-a");
        assert_eq!(stateful.status, StatefulWorkflowRunStatus::AwaitingApproval);
        assert_eq!(stateful.phase, StatefulWorkflowPhase::AwaitingApproval);
        assert_eq!(
            stateful.allowed_next_phases,
            StatefulWorkflowPhase::AwaitingApproval
                .allowed_next_phases()
                .to_vec()
        );
        assert_eq!(stateful.phase_history.len(), 1);
        assert_eq!(
            stateful.phase_history[0].phase_id.as_deref(),
            Some("approve-plan")
        );
        assert_eq!(stateful.active_wait_kind, Some(StatefulWaitKind::Approval));
        assert_eq!(stateful.active_wait_id.as_deref(), Some("approve-plan"));
        assert_eq!(stateful.related_context_run_ids, vec!["context-run-a"]);
        assert_eq!(
            stateful.workflow_definition_version.as_deref(),
            Some("release-17")
        );
        assert_eq!(
            stateful.workflow_definition_snapshot_hash.as_deref(),
            Some(expected_hash.as_str())
        );
    }

    #[test]
    fn automation_adapter_preserves_webhook_enterprise_scope_metadata() {
        let checkpoint = AutomationRunCheckpoint {
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
        let mut snapshot = automation_snapshot("automation-a");
        let resource_scope = ResourceScope::root(ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::Repository,
            "repo-a",
        ));
        snapshot.metadata = Some(json!({
            "automation_webhook": {
                "owner_principal": PrincipalRef::new(PrincipalKind::Automation, "automation-a"),
                "owning_org_unit_id": "finance",
                "resource_scope": resource_scope,
                "data_class": DataClass::FinancialRecord,
                "risk_tier": ToolRiskTier::FinancialRecordAccess,
                "policy_version_id": "policy-2026-06",
                "delegation_grant_ids": ["delegation-a"]
            }
        }));
        let record = automation_run(Some(snapshot), checkpoint);

        let stateful = stateful_run_from_automation_v2(&record);

        assert_eq!(
            stateful
                .scope
                .owner_principal
                .as_ref()
                .map(|principal| principal.id.as_str()),
            Some("automation-a")
        );
        assert_eq!(
            stateful.scope.owning_org_unit_id.as_deref(),
            Some("finance")
        );
        assert_eq!(
            stateful
                .scope
                .resource_scope
                .as_ref()
                .map(|scope| scope.root.resource_id.as_str()),
            Some("repo-a")
        );
        assert_eq!(
            stateful.scope.data_classes,
            vec![DataClass::FinancialRecord]
        );
        assert_eq!(
            stateful.scope.risk_tier,
            Some(ToolRiskTier::FinancialRecordAccess)
        );
        assert_eq!(
            stateful.scope.policy_version_id.as_deref(),
            Some("policy-2026-06")
        );
        assert_eq!(stateful.scope.delegation_grant_ids, vec!["delegation-a"]);
    }

    #[test]
    fn automation_adapter_derives_distinct_fallback_versions_from_snapshot_hashes() {
        let mut first_snapshot = automation_snapshot("automation-a");
        first_snapshot.metadata = None;
        let mut second_snapshot = first_snapshot.clone();
        second_snapshot.name = "Definition test changed".to_string();

        let checkpoint = AutomationRunCheckpoint {
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
        let first_run = automation_run(Some(first_snapshot), checkpoint.clone());
        let second_run = automation_run(Some(second_snapshot), checkpoint);

        let first_stateful = stateful_run_from_automation_v2(&first_run);
        let second_stateful = stateful_run_from_automation_v2(&second_run);

        assert_ne!(
            first_stateful.workflow_definition_snapshot_hash,
            second_stateful.workflow_definition_snapshot_hash
        );
        assert_ne!(
            first_stateful.workflow_definition_version,
            second_stateful.workflow_definition_version
        );
        assert!(first_stateful
            .workflow_definition_version
            .as_deref()
            .is_some_and(|version| version.starts_with("automation:automation-a@")));
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
        assert_eq!(stateful.phase, StatefulWorkflowPhase::RunningPhase);
        assert!(stateful
            .allowed_next_phases
            .contains(&StatefulWorkflowPhase::AwaitingApproval));
    }
}
