use super::*;

use crate::audit::append_protected_audit_event;
use crate::automation_v2::types::{
    AutomationPendingGate, AutomationRunCheckpoint, AutomationRunStatus, AutomationV2RunRecord,
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
use tandem_types::{PolicyDecisionEffect, PolicyDecisionRecord, TenantContext};

fn tenant(org_id: &str, workspace_id: &str, actor_id: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org_id, workspace_id, None, actor_id)
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&body).expect("response json")
}

#[tokio::test]
async fn run_observability_contract_joins_governance_waits_and_reliability() {
    let state = test_state().await;
    let tenant = tenant("org-observability", "workspace-observability", "operator-a");
    let run_id = "run-observability";
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());

    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), automation_run(run_id, tenant.clone()));
    upsert_stateful_wait(&paths.waits_path, wait(run_id, scope.clone()))
        .await
        .expect("upsert wait");
    append_stateful_run_event(&paths.run_events_path, &event(run_id, scope.clone()))
        .await
        .expect("append event");
    write_stateful_run_snapshot(&paths.snapshots_root, &snapshot(run_id, scope.clone()))
        .await
        .expect("write snapshot");
    upsert_stateful_outbox(&reliability_path, outbox(run_id, scope.clone()))
        .await
        .expect("upsert outbox");
    upsert_stateful_tool_effect(&reliability_path, tool_effect(run_id, scope.clone()))
        .await
        .expect("upsert tool effect");
    upsert_stateful_dead_letter(&reliability_path, dead_letter(run_id, scope.clone()))
        .await
        .expect("upsert dead letter");
    upsert_stateful_compensation(&reliability_path, compensation(run_id, scope.clone()))
        .await
        .expect("upsert compensation");
    state
        .record_policy_decision(policy_decision(
            "decision-observability",
            tenant.clone(),
            run_id,
        ))
        .await
        .expect("record policy decision");
    append_protected_audit_event(
        &state,
        "policy.decision.recorded",
        &tenant,
        Some("operator-a".to_string()),
        json!({
            "run_id": run_id,
            "policy_decision_id": "decision-observability",
            "raw_payload": "must not be returned",
        }),
    )
    .await
    .expect("append protected audit");

    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/stateful-runtime/runs/{run_id}/observability?event_limit=5&snapshot_limit=5"
                ))
                .header("x-tandem-org-id", tenant.org_id.as_str())
                .header("x-tandem-workspace-id", tenant.workspace_id.as_str())
                .header("x-tandem-actor-id", "operator-a")
                .body(Body::empty())
                .expect("observability request"),
        )
        .await
        .expect("observability response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["source"], json!("stateful_runtime_observability"));
    assert_eq!(payload["run"]["run_id"], json!(run_id));
    assert_eq!(
        payload["current_wait"]["wait_id"],
        json!("wait-observability")
    );
    assert_eq!(payload["counts"]["waits"], json!(1));
    assert_eq!(payload["counts"]["policy_decisions"], json!(1));
    assert_eq!(payload["counts"]["tool_effects"], json!(1));
    assert_eq!(payload["counts"]["dead_letters"], json!(1));
    assert_eq!(payload["counts"]["compensations"], json!(1));
    assert_eq!(payload["counts"]["protected_audit_events"], json!(1));
    assert_eq!(
        payload["events"]["latest"]["correlation_id"],
        json!("effect-observability")
    );
    assert_eq!(
        payload["snapshots"]["latest"]["snapshot_id"],
        json!("snapshot-observability")
    );
    assert_eq!(
        payload["policy_decisions"][0]["decision"],
        json!("approval_required")
    );
    assert_eq!(
        payload["audit"]["payload_policy"],
        json!("redacted_payload_digest_only")
    );
    assert!(payload["audit"]["protected_events"][0]
        .get("raw_payload")
        .is_none());
    assert!(payload["operator_summary"]["is_blocked"]
        .as_bool()
        .unwrap_or(false));
    let action_names = payload["operator_summary"]["allowed_actions"]
        .as_array()
        .expect("actions")
        .iter()
        .filter_map(|action| action.get("action").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(action_names.contains(&"review_wait"));
    assert!(action_names.contains(&"approve_or_deny_policy_decision"));
    assert!(action_names.contains(&"retry_or_reconcile_effect"));
    assert!(action_names.contains(&"resolve_dead_letter"));
    assert!(action_names.contains(&"run_or_cancel_compensation"));
}

#[tokio::test]
async fn run_observability_falls_back_to_run_level_active_wait() {
    let state = test_state().await;
    let tenant = tenant("org-observability-wait", "workspace-wait", "operator-a");
    let run_id = "run-level-wait";
    let mut run = automation_run(run_id, tenant.clone());
    run.status = AutomationRunStatus::AwaitingApproval;
    run.checkpoint.awaiting_gate = Some(AutomationPendingGate {
        node_id: "approve-run-level".to_string(),
        title: "Approve run".to_string(),
        instructions: Some("review the run-level gate".to_string()),
        decisions: vec!["approve".to_string(), "reject".to_string()],
        rework_targets: Vec::new(),
        requested_at_ms: 2_250,
        upstream_node_ids: vec!["prepare".to_string()],
        metadata: None,
        expiry_policy: None,
    });
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), run);

    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/stateful-runtime/runs/{run_id}/observability"))
                .header("x-tandem-org-id", tenant.org_id.as_str())
                .header("x-tandem-workspace-id", tenant.workspace_id.as_str())
                .header("x-tandem-actor-id", "operator-a")
                .body(Body::empty())
                .expect("observability request"),
        )
        .await
        .expect("observability response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["counts"]["waits"], json!(0));
    assert_eq!(payload["counts"]["active_waits"], json!(1));
    assert_eq!(
        payload["current_wait"]["wait_id"],
        json!("approve-run-level")
    );
    assert_eq!(payload["current_wait"]["wait_kind"], json!("approval"));
    assert_eq!(
        payload["current_wait"]["metadata"]["source"],
        json!("run_record")
    );
    let action_names = payload["operator_summary"]["allowed_actions"]
        .as_array()
        .expect("actions")
        .iter()
        .filter_map(|action| action.get("action").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(action_names.contains(&"review_wait"));
}

#[tokio::test]
async fn run_observability_keeps_superseded_recovery_history_out_of_operator_summary() {
    let state = test_state().await;
    let tenant = tenant(
        "org-observability-superseded",
        "workspace-recovery",
        "operator-a",
    );
    let run_id = "run-observability-superseded";
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    let mut run = automation_run(run_id, tenant.clone());
    run.status = AutomationRunStatus::Running;
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), run);

    let mut superseded_dead_letter = dead_letter(run_id, scope.clone());
    superseded_dead_letter.metadata = Some(json!({
        "superseded_by_success": true,
        "superseded_by_effect_id": "effect-replayed",
        "superseded_at_ms": 3_000,
    }));
    upsert_stateful_dead_letter(&reliability_path, superseded_dead_letter)
        .await
        .expect("upsert superseded dead letter");
    let mut superseded_compensation = compensation(run_id, scope);
    superseded_compensation.metadata = Some(json!({
        "superseded_by_success": true,
        "superseded_by_effect_id": "effect-replayed",
        "superseded_at_ms": 3_000,
    }));
    upsert_stateful_compensation(&reliability_path, superseded_compensation)
        .await
        .expect("upsert superseded compensation");

    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/stateful-runtime/runs/{run_id}/observability"))
                .header("x-tandem-org-id", tenant.org_id.as_str())
                .header("x-tandem-workspace-id", tenant.workspace_id.as_str())
                .header("x-tandem-actor-id", "operator-a")
                .body(Body::empty())
                .expect("observability request"),
        )
        .await
        .expect("observability response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["counts"]["dead_letters"], json!(1));
    assert_eq!(payload["counts"]["compensations"], json!(1));
    assert_eq!(
        payload["reliability"]["dead_letters"][0]["dead_letter_id"],
        json!("dead-letter-observability")
    );
    assert_eq!(
        payload["reliability"]["compensations"][0]["compensation_id"],
        json!("compensation-observability")
    );
    assert_eq!(payload["operator_summary"]["is_blocked"], json!(false));
    assert!(payload["operator_summary"]["blocking_reasons"]
        .as_array()
        .expect("blocking reasons")
        .is_empty());
    assert!(payload["operator_summary"]["allowed_actions"]
        .as_array()
        .expect("allowed actions")
        .is_empty());
}

#[tokio::test]
async fn run_observability_tails_protected_audit_events() {
    let state = test_state().await;
    let tenant = tenant("org-observability-audit", "workspace-audit", "operator-a");
    let run_id = "run-audit-tail";
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), automation_run(run_id, tenant.clone()));
    append_protected_audit_event(
        &state,
        "tail.old",
        &tenant,
        Some("operator-a".to_string()),
        json!({ "run_id": run_id, "marker": "old" }),
    )
    .await
    .expect("append old protected audit");
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    append_protected_audit_event(
        &state,
        "tail.middle",
        &tenant,
        Some("operator-a".to_string()),
        json!({ "run_id": run_id, "marker": "middle" }),
    )
    .await
    .expect("append middle protected audit");
    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    append_protected_audit_event(
        &state,
        "tail.newest",
        &tenant,
        Some("operator-a".to_string()),
        json!({ "run_id": run_id, "marker": "newest" }),
    )
    .await
    .expect("append newest protected audit");

    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/stateful-runtime/runs/{run_id}/observability?audit_limit=2"
                ))
                .header("x-tandem-org-id", tenant.org_id.as_str())
                .header("x-tandem-workspace-id", tenant.workspace_id.as_str())
                .header("x-tandem-actor-id", "operator-a")
                .body(Body::empty())
                .expect("observability request"),
        )
        .await
        .expect("observability response");

    assert_eq!(response.status(), StatusCode::OK);
    let payload = response_json(response).await;
    assert_eq!(payload["counts"]["protected_audit_events"], json!(2));
    let event_types = payload["audit"]["protected_events"]
        .as_array()
        .expect("protected audit events")
        .iter()
        .filter_map(|event| event.get("event_type").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert_eq!(event_types, vec!["tail.middle", "tail.newest"]);
}

#[tokio::test]
async fn run_observability_contract_enforces_tenant_scope() {
    let state = test_state().await;
    let tenant_a = tenant("org-observability-a", "workspace-a", "operator-a");
    let tenant_b = tenant("org-observability-b", "workspace-b", "operator-b");
    state.automation_v2_runs.write().await.insert(
        "run-hidden".to_string(),
        automation_run("run-hidden", tenant_a),
    );

    let response = app_router(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/stateful-runtime/runs/run-hidden/observability")
                .header("x-tandem-org-id", tenant_b.org_id.as_str())
                .header("x-tandem-workspace-id", tenant_b.workspace_id.as_str())
                .header("x-tandem-actor-id", "operator-b")
                .body(Body::empty())
                .expect("observability request"),
        )
        .await
        .expect("observability response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let payload = response_json(response).await;
    assert_eq!(payload["error"], json!("stateful_run_not_found"));
}

fn automation_run(run_id: &str, tenant_context: TenantContext) -> AutomationV2RunRecord {
    AutomationV2RunRecord {
        run_id: run_id.to_string(),
        automation_id: "automation-observability".to_string(),
        tenant_context,
        trigger_type: "webhook".to_string(),
        status: AutomationRunStatus::Paused,
        created_at_ms: 1_000,
        updated_at_ms: 2_000,
        started_at_ms: Some(1_050),
        finished_at_ms: None,
        active_session_ids: Vec::new(),
        latest_session_id: None,
        active_instance_ids: Vec::new(),
        checkpoint: AutomationRunCheckpoint {
            completed_nodes: vec!["node-complete".to_string()],
            pending_nodes: vec!["node-pending".to_string()],
            node_outputs: Default::default(),
            node_attempts: Default::default(),
            node_attempt_verdicts: Default::default(),
            blocked_nodes: vec!["node-blocked".to_string()],
            awaiting_gate: None,
            gate_history: Vec::new(),
            lifecycle_history: Vec::new(),
            last_failure: None,
        },
        runtime_context: None,
        automation_snapshot: None,
        workflow_definition_version: Some("v1".to_string()),
        workflow_definition_snapshot_hash: Some("sha256:definition".to_string()),
        execution_claim: None,
        execution_claim_epoch: 0,
        pause_reason: Some("operator_review".to_string()),
        resume_reason: None,
        detail: Some("paused for observability contract".to_string()),
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

fn wait(run_id: &str, scope: StatefulRuntimeScope) -> StatefulWaitRecord {
    StatefulWaitRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        wait_id: "wait-observability".to_string(),
        run_id: run_id.to_string(),
        wait_kind: StatefulWaitKind::Approval,
        status: StatefulWaitStatus::Waiting,
        scope,
        phase_id: Some("phase-review".to_string()),
        reason: Some("awaiting operator approval".to_string()),
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
        metadata: Some(json!({ "approval_id": "approval-observability" })),
    }
}

fn event(run_id: &str, scope: StatefulRuntimeScope) -> StatefulRunEventRecord {
    StatefulRunEventRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        event_id: "event-observability".to_string(),
        run_id: run_id.to_string(),
        seq: 7,
        event_type: "runtime.wait.recorded".to_string(),
        occurred_at_ms: 1_250,
        scope,
        actor: None,
        phase_id: Some("phase-review".to_string()),
        phase_transition: None,
        wait_kind: Some(StatefulWaitKind::Approval),
        causation_id: Some("wait-observability".to_string()),
        correlation_id: Some("effect-observability".to_string()),
        payload: json!({ "run_id": run_id, "wait_id": "wait-observability" }),
    }
}

fn snapshot(run_id: &str, scope: StatefulRuntimeScope) -> StatefulRunSnapshotRecord {
    StatefulRunSnapshotRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        snapshot_id: "snapshot-observability".to_string(),
        run_id: run_id.to_string(),
        seq: 7,
        created_at_ms: 1_300,
        scope,
        status: StatefulWorkflowRunStatus::Paused,
        phase: StatefulWorkflowPhase::default(),
        phase_history: Vec::new(),
        allowed_next_phases: Vec::new(),
        phase_id: Some("phase-review".to_string()),
        source_record_kind: None,
        checkpoint: Some(json!({ "blocked_nodes": ["node-blocked"] })),
        payload_digest: Some("sha256:snapshot".to_string()),
        workflow_definition_version: Some("v1".to_string()),
        workflow_definition_snapshot_hash: Some("sha256:definition".to_string()),
        metadata: None,
    }
}

fn outbox(run_id: &str, scope: StatefulRuntimeScope) -> StatefulOutboxRecord {
    StatefulOutboxRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        outbox_id: "outbox-observability".to_string(),
        run_id: Some(run_id.to_string()),
        scope,
        operation: "send_webhook_reply".to_string(),
        status: StatefulOutboxStatus::Failed,
        source_kind: Some("automation_v2".to_string()),
        source_id: Some("node-effect".to_string()),
        node_id: Some("node-effect".to_string()),
        provider: Some("github".to_string()),
        tool: Some("github.comment".to_string()),
        target: Some("repo-1".to_string()),
        idempotency_key: Some("idem-observability".to_string()),
        payload_digest: Some("sha256:payload".to_string()),
        policy_decision_id: Some("decision-observability".to_string()),
        context_assertion_id: None,
        effect_id: Some("effect-observability".to_string()),
        receipt_id: None,
        compensation_id: Some("compensation-observability".to_string()),
        dead_letter_id: Some("dead-letter-observability".to_string()),
        attempts: 2,
        created_at_ms: 1_350,
        updated_at_ms: 1_450,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        metadata: None,
    }
}

fn tool_effect(run_id: &str, scope: StatefulRuntimeScope) -> StatefulToolEffectRecord {
    StatefulToolEffectRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        effect_id: "effect-observability".to_string(),
        outbox_id: Some("outbox-observability".to_string()),
        action_id: Some("action-observability".to_string()),
        run_id: Some(run_id.to_string()),
        scope,
        status: StatefulToolEffectStatus::Failed,
        operation: "github.comment".to_string(),
        source_kind: Some("automation_v2".to_string()),
        source_id: Some("node-effect".to_string()),
        node_id: Some("node-effect".to_string()),
        provider: Some("github".to_string()),
        tool: Some("github.comment".to_string()),
        target: Some("repo-1".to_string()),
        external_resource: None,
        policy_decision_id: Some("decision-observability".to_string()),
        context_assertion_id: None,
        input_digest: Some("sha256:input".to_string()),
        output_digest: None,
        receipt_payload_digest: None,
        receipt_payload_redacted: None,
        receipt_pointer: None,
        redaction_tier: "metadata_only".to_string(),
        audit_hash: "sha256:audit".to_string(),
        error: Some("provider timeout".to_string()),
        compensation_id: Some("compensation-observability".to_string()),
        created_at_ms: 1_350,
        updated_at_ms: 1_450,
        metadata: None,
    }
}

fn dead_letter(run_id: &str, scope: StatefulRuntimeScope) -> StatefulDeadLetterRecord {
    StatefulDeadLetterRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        dead_letter_id: "dead-letter-observability".to_string(),
        source_type: "tool_effect".to_string(),
        source_id: "effect-observability".to_string(),
        run_id: Some(run_id.to_string()),
        scope,
        reason: "provider timeout".to_string(),
        status: StatefulDeadLetterStatus::Open,
        recovery_options: vec![
            StatefulRecoveryOption::Retry,
            StatefulRecoveryOption::Compensate,
        ],
        payload_pointer: None,
        compensation_id: Some("compensation-observability".to_string()),
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

fn compensation(run_id: &str, scope: StatefulRuntimeScope) -> StatefulCompensationRecord {
    StatefulCompensationRecord {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        compensation_id: "compensation-observability".to_string(),
        run_id: Some(run_id.to_string()),
        scope,
        status: StatefulCompensationStatus::AwaitingApproval,
        compensation_type: "operator_review".to_string(),
        target_effect_id: Some("effect-observability".to_string()),
        outbox_id: Some("outbox-observability".to_string()),
        approval_required: true,
        policy_decision_id: Some("decision-observability".to_string()),
        rollback_instruction: Some("skip duplicate comment".to_string()),
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
        requester_context: None,
        actor_id: Some("operator-a".to_string()),
        session_id: None,
        message_id: None,
        run_id: Some(run_id.to_string()),
        automation_id: Some("automation-observability".to_string()),
        node_id: Some("node-effect".to_string()),
        tool: Some("github.comment".to_string()),
        resource: None,
        data_classes: Vec::new(),
        risk_tier: Some("external_effect".to_string()),
        decision: PolicyDecisionEffect::ApprovalRequired,
        reason_code: "approval_required_external_effect".to_string(),
        reason: "external effect requires approval".to_string(),
        policy_id: Some("policy-observability".to_string()),
        grant_id: None,
        approval_id: Some("approval-observability".to_string()),
        audit_event_id: None,
        created_at_ms: 1_340,
        metadata: json!({ "contract_fixture": true }),
    }
}
