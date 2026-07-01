use super::*;

use crate::app::state::AppState;
use crate::audit::append_protected_audit_event;
use crate::automation_v2::types::{
    AutomationRunCheckpoint, AutomationRunStatus, AutomationV2RunRecord,
};
use crate::stateful_runtime::{
    append_stateful_run_event, stateful_reliability_path_from_runtime_events_path,
    upsert_stateful_compensation, upsert_stateful_dead_letter, upsert_stateful_outbox,
    upsert_stateful_tool_effect, upsert_stateful_wait, write_stateful_run_snapshot,
    StatefulCompensationRecord, StatefulCompensationStatus, StatefulDeadLetterRecord,
    StatefulDeadLetterStatus, StatefulOutboxRecord, StatefulOutboxStatus, StatefulRecoveryOption,
    StatefulRunEventRecord, StatefulRunSnapshotRecord, StatefulRuntimeScope,
    StatefulRuntimeStoragePaths, StatefulToolEffectRecord, StatefulToolEffectStatus,
    StatefulWaitKind, StatefulWaitRecord, StatefulWaitStatus, StatefulWorkflowPhase,
    StatefulWorkflowRunStatus, STATEFUL_RUNTIME_SCHEMA_VERSION,
};
use serde_json::{json, Value};
use tandem_types::{
    DataClass, PolicyDecisionEffect, PolicyDecisionRecord, PrincipalKind, PrincipalRef,
    ResourceKind, ResourceRef, ResourceScope, TenantContext, ToolRiskTier,
};
use tandem_workflows::{WorkflowRunRecord, WorkflowRunStatus};

fn tenant(org_id: &str, workspace_id: &str, actor_id: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org_id, workspace_id, None, actor_id)
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&body).expect("response json")
}

async fn get_json(state: AppState, uri: impl Into<String>, tenant: &TenantContext) -> Value {
    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri.into())
                .header("x-tandem-org-id", tenant.org_id.as_str())
                .header("x-tandem-workspace-id", tenant.workspace_id.as_str())
                .header(
                    "x-tandem-actor-id",
                    tenant.actor_id.as_deref().unwrap_or("operator"),
                )
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
    response_json(response).await
}

#[tokio::test]
async fn stateful_runtime_read_models_filter_cross_tenant_rows_with_shared_run_id() {
    let state = test_state().await;
    let tenant_a = tenant("org-hardening-a", "workspace-a", "operator-a");
    let tenant_b = tenant("org-hardening-b", "workspace-b", "operator-b");
    let run_id = "run-hardening-shared";
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let scope_a = scoped_runtime(&tenant_a, "finance", "repo-finance", "grant-finance");
    let scope_b = scoped_runtime(&tenant_b, "legal", "repo-legal", "grant-legal");

    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), automation_run(run_id, tenant_a.clone()));
    upsert_stateful_wait(
        &paths.waits_path,
        wait_record("wait-visible", run_id, scope_a.clone()),
    )
    .await
    .expect("visible wait");
    upsert_stateful_wait(
        &paths.waits_path,
        wait_record("wait-hidden", run_id, scope_b.clone()),
    )
    .await
    .expect("hidden wait");
    append_stateful_run_event(
        &paths.run_events_path,
        &event_record("event-visible", run_id, 1, scope_a.clone()),
    )
    .await
    .expect("visible event");
    append_stateful_run_event(
        &paths.run_events_path,
        &event_record("event-hidden", run_id, 2, scope_b.clone()),
    )
    .await
    .expect("hidden event");
    write_stateful_run_snapshot(
        &paths.snapshots_root,
        &snapshot_record("snapshot-visible", run_id, 1, scope_a.clone()),
    )
    .await
    .expect("visible snapshot");
    write_stateful_run_snapshot(
        &paths.snapshots_root,
        &snapshot_record("snapshot-hidden", run_id, 2, scope_b.clone()),
    )
    .await
    .expect("hidden snapshot");
    upsert_stateful_outbox(
        &reliability_path,
        outbox_record("outbox-visible", run_id, scope_a.clone()),
    )
    .await
    .expect("visible outbox");
    upsert_stateful_outbox(
        &reliability_path,
        outbox_record("outbox-hidden", run_id, scope_b.clone()),
    )
    .await
    .expect("hidden outbox");
    upsert_stateful_tool_effect(
        &reliability_path,
        tool_effect_record("effect-visible", run_id, scope_a.clone()),
    )
    .await
    .expect("visible effect");
    upsert_stateful_tool_effect(
        &reliability_path,
        tool_effect_record("effect-hidden", run_id, scope_b.clone()),
    )
    .await
    .expect("hidden effect");
    upsert_stateful_dead_letter(
        &reliability_path,
        dead_letter_record("dead-visible", run_id, scope_a.clone()),
    )
    .await
    .expect("visible dead letter");
    upsert_stateful_dead_letter(
        &reliability_path,
        dead_letter_record("dead-hidden", run_id, scope_b.clone()),
    )
    .await
    .expect("hidden dead letter");
    upsert_stateful_compensation(
        &reliability_path,
        compensation_record("comp-visible", "effect-visible", run_id, scope_a.clone()),
    )
    .await
    .expect("visible compensation");
    upsert_stateful_compensation(
        &reliability_path,
        compensation_record("comp-hidden", "effect-hidden", run_id, scope_b.clone()),
    )
    .await
    .expect("hidden compensation");
    state
        .record_policy_decision(policy_decision(
            "decision-visible",
            tenant_a.clone(),
            run_id,
        ))
        .await
        .expect("visible policy decision");
    state
        .record_policy_decision(policy_decision("decision-hidden", tenant_b.clone(), run_id))
        .await
        .expect("hidden policy decision");
    append_protected_audit_event(
        &state,
        "audit.visible",
        &tenant_a,
        tenant_a.actor_id.clone(),
        json!({ "run_id": run_id, "decision_id": "decision-visible" }),
    )
    .await
    .expect("visible protected audit");
    append_protected_audit_event(
        &state,
        "audit.hidden",
        &tenant_b,
        tenant_b.actor_id.clone(),
        json!({ "run_id": run_id, "decision_id": "decision-hidden" }),
    )
    .await
    .expect("hidden protected audit");

    let payload = get_json(
        state.clone(),
        format!(
            "/stateful-runtime/runs/{run_id}/observability?event_limit=10&snapshot_limit=10&reliability_limit=10&audit_limit=10"
        ),
        &tenant_a,
    )
    .await;
    assert_eq!(payload["counts"]["waits"], json!(1));
    assert_eq!(payload["counts"]["events"], json!(1));
    assert_eq!(payload["counts"]["snapshots"], json!(1));
    assert_eq!(payload["counts"]["policy_decisions"], json!(1));
    assert_eq!(payload["counts"]["outbox"], json!(1));
    assert_eq!(payload["counts"]["tool_effects"], json!(1));
    assert_eq!(payload["counts"]["dead_letters"], json!(1));
    assert_eq!(payload["counts"]["compensations"], json!(1));
    assert_eq!(payload["counts"]["protected_audit_events"], json!(1));
    assert_payload_excludes_hidden_runtime_rows(&payload);

    let events = get_json(
        state.clone(),
        format!("/stateful-runtime/runs/{run_id}/events?limit=10"),
        &tenant_a,
    )
    .await;
    assert_eq!(events["count"], json!(1));
    assert_eq!(events["events"][0]["event_id"], json!("event-visible"));

    let snapshots = get_json(
        state.clone(),
        format!("/stateful-runtime/runs/{run_id}/snapshots?limit=10"),
        &tenant_a,
    )
    .await;
    assert_eq!(snapshots["count"], json!(1));
    assert_eq!(
        snapshots["snapshots"][0]["snapshot_id"],
        json!("snapshot-visible")
    );

    let reliability = get_json(
        state,
        format!("/stateful-runtime/runs/{run_id}/reliability?limit=10"),
        &tenant_a,
    )
    .await;
    assert_eq!(reliability["counts"]["outbox"], json!(1));
    assert_eq!(reliability["counts"]["tool_effects"], json!(1));
    assert_eq!(reliability["counts"]["dead_letters"], json!(1));
    assert_eq!(reliability["counts"]["compensations"], json!(1));
    assert_payload_excludes_hidden_runtime_rows(&reliability);
}

#[tokio::test]
async fn stateful_runtime_enterprise_scope_filters_are_tenant_scoped() {
    let state = test_state().await;
    let tenant_a = tenant("org-enterprise-a", "workspace-a", "operator-a");
    let tenant_b = tenant("org-enterprise-b", "workspace-b", "operator-b");
    insert_workflow_run(
        &state,
        workflow_run(
            "run-finance",
            tenant_a.clone(),
            enterprise_scope(
                "org-enterprise-a",
                "workspace-a",
                "finance",
                "repo-finance",
                "grant-finance",
            ),
        ),
    )
    .await;
    insert_workflow_run(
        &state,
        workflow_run(
            "run-platform",
            tenant_a.clone(),
            enterprise_scope(
                "org-enterprise-a",
                "workspace-a",
                "platform",
                "repo-platform",
                "grant-platform",
            ),
        ),
    )
    .await;
    insert_workflow_run(
        &state,
        workflow_run(
            "run-other-tenant",
            tenant_b,
            enterprise_scope(
                "org-enterprise-b",
                "workspace-b",
                "finance",
                "repo-finance",
                "grant-finance",
            ),
        ),
    )
    .await;

    let payload = get_json(
        state.clone(),
        "/stateful-runtime/runs?org_unit_id=finance&data_class=financial_record&delegation_grant_id=grant-finance&resource_kind=repository&resource_id=repo-finance&policy_version_id=policy-finance",
        &tenant_a,
    )
    .await;
    assert_eq!(payload["count"], json!(1));
    assert_eq!(payload["runs"][0]["run"]["run_id"], json!("run-finance"));
    assert_eq!(
        payload["runs"][0]["enterprise_scope"]["owning_org_unit_id"],
        json!("finance")
    );
    assert_eq!(
        payload["runs"][0]["enterprise_scope"]["summary"]["delegation_grant_count"],
        json!(1)
    );

    let denied = get_json(
        state.clone(),
        "/stateful-runtime/runs?org_unit_id=finance&data_class=credential",
        &tenant_a,
    )
    .await;
    assert_eq!(denied["count"], json!(0));

    let detail = get_json(state, "/stateful-runtime/runs/run-finance", &tenant_a).await;
    assert_eq!(
        detail["enterprise_scope"]["resource_scope"]["root"]["resource_id"],
        json!("repo-finance")
    );
    assert_eq!(
        detail["enterprise_scope"]["policy_version_id"],
        json!("policy-finance")
    );
    assert_eq!(
        detail["enterprise_scope"]["delegation_grant_ids"],
        json!(["grant-finance"])
    );
}

#[tokio::test]
async fn stateful_runtime_resume_plan_surfaces_partial_failure_without_cross_tenant_rows() {
    let state = test_state().await;
    let tenant_a = tenant("org-recovery-a", "workspace-a", "operator-a");
    let tenant_b = tenant("org-recovery-b", "workspace-b", "operator-b");
    let run_id = "run-partial-failure";
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let scope_a = scoped_runtime(&tenant_a, "finance", "repo-finance", "grant-finance");
    let scope_b = scoped_runtime(&tenant_b, "finance", "repo-finance", "grant-finance");

    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), automation_run(run_id, tenant_a.clone()));
    upsert_stateful_tool_effect(
        &reliability_path,
        succeeded_effect_record("effect-sent", run_id, scope_a.clone()),
    )
    .await
    .expect("completed effect");
    upsert_stateful_tool_effect(
        &reliability_path,
        tool_effect_record("effect-failed", run_id, scope_a.clone()),
    )
    .await
    .expect("uncertain effect");
    upsert_stateful_tool_effect(
        &reliability_path,
        tool_effect_record("effect-hidden", run_id, scope_b.clone()),
    )
    .await
    .expect("hidden effect");
    upsert_stateful_dead_letter(
        &reliability_path,
        dead_letter_record("dead-failed", run_id, scope_a.clone()),
    )
    .await
    .expect("dead letter");
    upsert_stateful_dead_letter(
        &reliability_path,
        dead_letter_record("dead-hidden", run_id, scope_b.clone()),
    )
    .await
    .expect("hidden dead letter");
    upsert_stateful_compensation(
        &reliability_path,
        compensation_record("comp-failed", "effect-failed", run_id, scope_a.clone()),
    )
    .await
    .expect("compensation");
    upsert_stateful_compensation(
        &reliability_path,
        compensation_record("comp-hidden", "effect-hidden", run_id, scope_b),
    )
    .await
    .expect("hidden compensation");

    let plan = get_json(
        state,
        format!("/stateful-runtime/runs/{run_id}/resume-plan?limit=10"),
        &tenant_a,
    )
    .await;
    assert_eq!(plan["audit_summary"]["completed_effect_count"], json!(1));
    assert_eq!(plan["audit_summary"]["uncertain_effect_count"], json!(1));
    assert_eq!(plan["audit_summary"]["dead_letter_count"], json!(1));
    assert_eq!(
        plan["audit_summary"]["pending_compensation_count"],
        json!(1)
    );
    assert!(plan["operator_choices"]
        .as_array()
        .expect("operator choices")
        .iter()
        .any(|choice| choice["choice"] == "resume_from_checkpoint"));
    assert_payload_excludes_hidden_runtime_rows(&plan);
}

fn assert_payload_excludes_hidden_runtime_rows(payload: &Value) {
    let body = payload.to_string();
    for hidden in [
        "wait-hidden",
        "event-hidden",
        "snapshot-hidden",
        "outbox-hidden",
        "effect-hidden",
        "dead-hidden",
        "comp-hidden",
        "decision-hidden",
        "audit.hidden",
    ] {
        assert!(!body.contains(hidden), "payload leaked {hidden}: {body}");
    }
}

fn scoped_runtime(
    tenant: &TenantContext,
    org_unit_id: &str,
    resource_id: &str,
    delegation_grant_id: &str,
) -> StatefulRuntimeScope {
    let mut scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    scope.owner_principal = Some(PrincipalRef::new(
        PrincipalKind::Automation,
        "automation-hardening",
    ));
    scope.owning_org_unit_id = Some(org_unit_id.to_string());
    scope.resource_scope = Some(ResourceScope::root(ResourceRef::new(
        tenant.org_id.clone(),
        tenant.workspace_id.clone(),
        ResourceKind::Repository,
        resource_id,
    )));
    scope.data_classes = vec![DataClass::FinancialRecord];
    scope.risk_tier = Some(ToolRiskTier::FinancialRecordAccess);
    scope.policy_version_id = Some(format!("policy-{org_unit_id}"));
    scope.delegation_grant_ids = vec![delegation_grant_id.to_string()];
    scope
}

fn enterprise_scope(
    org_id: &str,
    workspace_id: &str,
    org_unit_id: &str,
    resource_id: &str,
    delegation_grant_id: &str,
) -> Value {
    json!({
        "owner_principal": PrincipalRef::new(PrincipalKind::Automation, "automation-hardening"),
        "owning_org_unit_id": org_unit_id,
        "resource_scope": ResourceScope::root(ResourceRef::new(
            org_id,
            workspace_id,
            ResourceKind::Repository,
            resource_id,
        )),
        "data_classes": [DataClass::FinancialRecord],
        "risk_tier": ToolRiskTier::FinancialRecordAccess,
        "policy_version_id": format!("policy-{org_unit_id}"),
        "delegation_grant_ids": [delegation_grant_id],
    })
}

async fn insert_workflow_run(state: &AppState, run: WorkflowRunRecord) {
    state
        .workflow_runs
        .write()
        .await
        .insert(run.run_id.clone(), run);
}

fn workflow_run(
    run_id: &str,
    tenant_context: TenantContext,
    enterprise_scope: Value,
) -> WorkflowRunRecord {
    WorkflowRunRecord {
        run_id: run_id.to_string(),
        workflow_id: "workflow-hardening".to_string(),
        tenant_context,
        automation_id: Some("automation-hardening".to_string()),
        automation_run_id: None,
        binding_id: None,
        trigger_event: Some("manual".to_string()),
        source_event_id: None,
        task_id: None,
        enterprise_scope: Some(enterprise_scope),
        status: WorkflowRunStatus::Running,
        created_at_ms: 1_000,
        updated_at_ms: 2_000,
        finished_at_ms: None,
        actions: Vec::new(),
        awaiting_gate: None,
        gate_history: Vec::new(),
        source: None,
    }
}

fn automation_run(run_id: &str, tenant_context: TenantContext) -> AutomationV2RunRecord {
    AutomationV2RunRecord {
        run_id: run_id.to_string(),
        automation_id: "automation-hardening".to_string(),
        tenant_context,
        trigger_type: "webhook".to_string(),
        status: AutomationRunStatus::Failed,
        created_at_ms: 1_000,
        updated_at_ms: 2_000,
        started_at_ms: Some(1_050),
        finished_at_ms: None,
        active_session_ids: Vec::new(),
        latest_session_id: None,
        active_instance_ids: Vec::new(),
        checkpoint: AutomationRunCheckpoint {
            completed_nodes: vec!["node-sent".to_string()],
            pending_nodes: vec!["node-retry".to_string()],
            node_outputs: Default::default(),
            node_attempts: Default::default(),
            node_attempt_verdicts: Default::default(),
            blocked_nodes: vec!["node-failed".to_string()],
            awaiting_gate: None,
            gate_history: Vec::new(),
            lifecycle_history: Vec::new(),
            last_failure: None,
        },
        runtime_context: None,
        automation_snapshot: None,
        workflow_definition_version: Some("v-hardening".to_string()),
        workflow_definition_snapshot_hash: Some("sha256:hardening".to_string()),
        execution_claim: None,
        execution_claim_epoch: 0,
        pause_reason: None,
        resume_reason: None,
        detail: Some("partial failure hardening fixture".to_string()),
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

fn wait_record(wait_id: &str, run_id: &str, scope: StatefulRuntimeScope) -> StatefulWaitRecord {
    StatefulWaitRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        wait_id: wait_id.to_string(),
        run_id: run_id.to_string(),
        wait_kind: StatefulWaitKind::Approval,
        status: StatefulWaitStatus::Waiting,
        scope,
        phase_id: Some("phase-review".to_string()),
        reason: Some(wait_id.to_string()),
        created_at_ms: 1_100,
        updated_at_ms: 1_200,
        wake_at_ms: None,
        timeout_policy: None,
        event_seq: None,
        wake_idempotency_key: None,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        completed_at_ms: None,
        metadata: None,
    }
}

fn event_record(
    event_id: &str,
    run_id: &str,
    seq: u64,
    scope: StatefulRuntimeScope,
) -> StatefulRunEventRecord {
    StatefulRunEventRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        event_id: event_id.to_string(),
        run_id: run_id.to_string(),
        seq,
        event_type: event_id.to_string(),
        occurred_at_ms: 1_250 + seq,
        scope,
        actor: None,
        phase_id: Some("phase-review".to_string()),
        phase_transition: None,
        wait_kind: Some(StatefulWaitKind::Approval),
        causation_id: None,
        correlation_id: None,
        payload: json!({ "run_id": run_id, "event_id": event_id }),
    }
}

fn snapshot_record(
    snapshot_id: &str,
    run_id: &str,
    seq: u64,
    scope: StatefulRuntimeScope,
) -> StatefulRunSnapshotRecord {
    StatefulRunSnapshotRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        snapshot_id: snapshot_id.to_string(),
        run_id: run_id.to_string(),
        seq,
        created_at_ms: 1_300 + seq,
        scope,
        status: StatefulWorkflowRunStatus::Failed,
        phase: StatefulWorkflowPhase::default(),
        phase_history: Vec::new(),
        allowed_next_phases: Vec::new(),
        phase_id: Some("phase-review".to_string()),
        source_record_kind: None,
        checkpoint: Some(json!({ "snapshot_id": snapshot_id })),
        payload_digest: Some(format!("sha256:{snapshot_id}")),
        workflow_definition_version: Some("v-hardening".to_string()),
        workflow_definition_snapshot_hash: Some("sha256:hardening".to_string()),
        metadata: None,
    }
}

fn outbox_record(
    outbox_id: &str,
    run_id: &str,
    scope: StatefulRuntimeScope,
) -> StatefulOutboxRecord {
    StatefulOutboxRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        outbox_id: outbox_id.to_string(),
        run_id: Some(run_id.to_string()),
        scope,
        operation: "github.comment".to_string(),
        status: StatefulOutboxStatus::Failed,
        source_kind: Some("automation_v2".to_string()),
        source_id: Some("node-effect".to_string()),
        node_id: Some("node-effect".to_string()),
        provider: Some("github".to_string()),
        tool: Some("github.comment".to_string()),
        target: Some("repo".to_string()),
        idempotency_key: Some(format!("idem-{outbox_id}")),
        payload_digest: Some("sha256:payload".to_string()),
        policy_decision_id: None,
        context_assertion_id: None,
        effect_id: Some(outbox_id.replace("outbox", "effect")),
        receipt_id: None,
        compensation_id: Some(outbox_id.replace("outbox", "comp")),
        dead_letter_id: Some(outbox_id.replace("outbox", "dead")),
        attempts: 2,
        created_at_ms: 1_350,
        updated_at_ms: 1_450,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        metadata: None,
    }
}

fn tool_effect_record(
    effect_id: &str,
    run_id: &str,
    scope: StatefulRuntimeScope,
) -> StatefulToolEffectRecord {
    StatefulToolEffectRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        effect_id: effect_id.to_string(),
        outbox_id: Some(effect_id.replace("effect", "outbox")),
        action_id: Some(format!("action-{effect_id}")),
        run_id: Some(run_id.to_string()),
        scope,
        status: StatefulToolEffectStatus::Failed,
        operation: "github.comment".to_string(),
        source_kind: Some("automation_v2".to_string()),
        source_id: Some("node-effect".to_string()),
        node_id: Some("node-effect".to_string()),
        provider: Some("github".to_string()),
        tool: Some("github.comment".to_string()),
        target: Some("repo".to_string()),
        external_resource: None,
        policy_decision_id: None,
        context_assertion_id: None,
        input_digest: Some("sha256:input".to_string()),
        output_digest: None,
        receipt_payload_digest: None,
        receipt_payload_redacted: None,
        receipt_pointer: None,
        redaction_tier: "metadata_only".to_string(),
        audit_hash: "sha256:audit".to_string(),
        error: Some("provider timeout".to_string()),
        compensation_id: Some(effect_id.replace("effect", "comp")),
        created_at_ms: 1_350,
        updated_at_ms: 1_450,
        metadata: None,
    }
}

fn succeeded_effect_record(
    effect_id: &str,
    run_id: &str,
    scope: StatefulRuntimeScope,
) -> StatefulToolEffectRecord {
    StatefulToolEffectRecord {
        status: StatefulToolEffectStatus::Succeeded,
        error: None,
        compensation_id: None,
        ..tool_effect_record(effect_id, run_id, scope)
    }
}

fn dead_letter_record(
    dead_letter_id: &str,
    run_id: &str,
    scope: StatefulRuntimeScope,
) -> StatefulDeadLetterRecord {
    StatefulDeadLetterRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        dead_letter_id: dead_letter_id.to_string(),
        source_type: "tool_effect".to_string(),
        source_id: dead_letter_id.replace("dead", "effect"),
        run_id: Some(run_id.to_string()),
        scope,
        reason: "provider timeout".to_string(),
        status: StatefulDeadLetterStatus::Open,
        recovery_options: vec![
            StatefulRecoveryOption::Retry,
            StatefulRecoveryOption::Compensate,
        ],
        payload_pointer: None,
        compensation_id: Some(dead_letter_id.replace("dead", "comp")),
        attempts: 2,
        created_at_ms: 1_450,
        updated_at_ms: 1_460,
        operator_disposition: None,
        disposition_reason: None,
        disposition_actor: None,
        disposition_at_ms: None,
        metadata: None,
    }
}

fn compensation_record(
    compensation_id: &str,
    target_effect_id: &str,
    run_id: &str,
    scope: StatefulRuntimeScope,
) -> StatefulCompensationRecord {
    StatefulCompensationRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        compensation_id: compensation_id.to_string(),
        run_id: Some(run_id.to_string()),
        scope,
        status: StatefulCompensationStatus::AwaitingApproval,
        compensation_type: "operator_review".to_string(),
        target_effect_id: Some(target_effect_id.to_string()),
        outbox_id: Some(target_effect_id.replace("effect", "outbox")),
        approval_required: true,
        policy_decision_id: None,
        rollback_instruction: Some("skip duplicate external mutation".to_string()),
        forward_fix_instruction: Some("retry after provider recovery".to_string()),
        receipt_effect_id: None,
        attempts: 0,
        created_at_ms: 1_460,
        updated_at_ms: 1_470,
        metadata: None,
    }
}

fn policy_decision(
    decision_id: &str,
    tenant_context: TenantContext,
    run_id: &str,
) -> PolicyDecisionRecord {
    PolicyDecisionRecord {
        decision_id: decision_id.to_string(),
        tenant_context,
        actor_id: Some("operator-a".to_string()),
        session_id: None,
        message_id: None,
        run_id: Some(run_id.to_string()),
        automation_id: Some("automation-hardening".to_string()),
        node_id: Some("node-effect".to_string()),
        tool: Some("github.comment".to_string()),
        resource: None,
        data_classes: vec![DataClass::FinancialRecord],
        risk_tier: Some("external_effect".to_string()),
        decision: PolicyDecisionEffect::ApprovalRequired,
        reason_code: "approval_required_external_effect".to_string(),
        reason: "external effect requires approval".to_string(),
        policy_id: Some("policy-hardening".to_string()),
        grant_id: None,
        approval_id: Some(format!("approval-{decision_id}")),
        audit_event_id: None,
        created_at_ms: 1_340,
        metadata: json!({ "hardening_fixture": true }),
    }
}
