use super::*;

use crate::app::state::AppState;
use crate::audit::append_protected_audit_event;
use crate::automation_v2::types::{
    AutomationRunCheckpoint, AutomationRunStatus, AutomationV2RunRecord,
};
use crate::stateful_runtime::{
    append_stateful_run_event, list_stateful_dead_letters,
    stateful_reliability_path_from_runtime_events_path, upsert_stateful_compensation,
    upsert_stateful_dead_letter, upsert_stateful_outbox, upsert_stateful_tool_effect,
    upsert_stateful_wait, write_stateful_run_snapshot, OrchestrationStateStore,
    StatefulCompensationRecord, StatefulCompensationStatus, StatefulDeadLetterRecord,
    StatefulDeadLetterStatus, StatefulOutboxRecord, StatefulOutboxStatus, StatefulRecoveryOption,
    StatefulReliabilityQuery, StatefulReliabilityStoreFile, StatefulRunEventRecord,
    StatefulRunSnapshotRecord, StatefulRuntimeScope, StatefulRuntimeStoragePaths,
    StatefulToolEffectRecord, StatefulToolEffectStatus, StatefulWaitKind, StatefulWaitRecord,
    StatefulWaitStatus, StatefulWorkflowPhase, StatefulWorkflowRunStatus,
    STATEFUL_RUNTIME_SCHEMA_VERSION,
};
use serde_json::{json, Value};
use tandem_enterprise_contract::{
    AccessPermission, OrganizationUnit, OrganizationUnitAccessGrant, OrganizationUnitKind,
};
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

#[test]
fn stateful_runtime_scope_local_implicit_does_not_see_explicit_tenant_rows() {
    let tenant_a = tenant("org-hardening-a", "workspace-a", "operator-a");
    let explicit_scope = StatefulRuntimeScope::from_tenant_context(tenant_a.clone());
    let local = TenantContext::local_implicit();

    assert!(explicit_scope.visible_to_tenant(&tenant_a));
    assert!(!explicit_scope.visible_to_tenant(&local));
    assert!(StatefulRuntimeScope::local_implicit().visible_to_tenant(&local));
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
    seed_runtime_delegation_grant(
        &state,
        &tenant_a,
        "finance",
        "repo-finance",
        "grant-finance",
    )
    .await;
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
async fn stateful_runtime_delegation_filter_requires_stored_grant_id() {
    let state = test_state().await;
    let tenant_a = tenant("org-enterprise-a", "workspace-a", "operator-a");
    seed_runtime_delegation_grant(
        &state,
        &tenant_a,
        "finance",
        "repo-finance",
        "grant-finance",
    )
    .await;
    let mut scope = enterprise_scope(
        "org-enterprise-a",
        "workspace-a",
        "finance",
        "repo-finance",
        "grant-finance",
    );
    scope
        .as_object_mut()
        .expect("enterprise scope object")
        .insert("delegation_grant_ids".to_string(), json!([]));
    insert_workflow_run(
        &state,
        workflow_run("run-without-grant", tenant_a.clone(), scope),
    )
    .await;

    let payload = get_json(
        state,
        "/stateful-runtime/runs?delegation_grant_id=grant-finance",
        &tenant_a,
    )
    .await;
    assert_eq!(payload["count"], json!(0));
}

#[tokio::test]
async fn stateful_runtime_grant_only_scope_resolves_active_authority() {
    let state = test_state().await;
    let tenant_a = tenant("org-enterprise-a", "workspace-a", "operator-a");
    seed_runtime_delegation_grant(
        &state,
        &tenant_a,
        "finance",
        "repo-finance",
        "grant-finance",
    )
    .await;
    let mut scope = enterprise_scope(
        "org-enterprise-a",
        "workspace-a",
        "finance",
        "repo-finance",
        "grant-finance",
    );
    scope
        .as_object_mut()
        .expect("enterprise scope object")
        .remove("owning_org_unit_id");
    insert_workflow_run(
        &state,
        workflow_run("run-grant-only", tenant_a.clone(), scope),
    )
    .await;

    let payload = get_json(
        state,
        "/stateful-runtime/runs?delegation_grant_id=grant-finance&resource_kind=repository&resource_id=repo-finance",
        &tenant_a,
    )
    .await;
    assert_eq!(payload["count"], json!(1));
    assert_eq!(payload["runs"][0]["run"]["run_id"], json!("run-grant-only"));
    assert_eq!(
        payload["runs"][0]["enterprise_scope"]["delegation_grant_authority"]["status"],
        json!("active")
    );
    assert_eq!(
        payload["runs"][0]["enterprise_scope"]["delegation_grant_authority"]["missing_grant_ids"],
        json!([])
    );
}

#[tokio::test]
async fn stateful_runtime_reliability_cursor_only_pages_matching_collection() {
    let state = test_state().await;
    let tenant_a = tenant("org-cursor-a", "workspace-a", "operator-a");
    let run_id = "run-reliability-cursor";
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let scope_a = scoped_runtime(&tenant_a, "finance", "repo-finance", "grant-finance");

    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), automation_run(run_id, tenant_a.clone()));
    upsert_stateful_outbox(
        &reliability_path,
        outbox_record("outbox-cursor", run_id, scope_a.clone()),
    )
    .await
    .expect("outbox");
    upsert_stateful_tool_effect(
        &reliability_path,
        tool_effect_record("effect-cursor", run_id, scope_a.clone()),
    )
    .await
    .expect("tool effect");
    upsert_stateful_compensation(
        &reliability_path,
        compensation_record("comp-cursor", "effect-cursor", run_id, scope_a.clone()),
    )
    .await
    .expect("compensation");
    let mut newer_dead_letter = dead_letter_record("dead-newer", run_id, scope_a.clone());
    newer_dead_letter.created_at_ms = 2_000;
    newer_dead_letter.updated_at_ms = 2_000;
    upsert_stateful_dead_letter(&reliability_path, newer_dead_letter)
        .await
        .expect("newer dead letter");
    let mut older_dead_letter = dead_letter_record("dead-older", run_id, scope_a);
    older_dead_letter.created_at_ms = 1_000;
    older_dead_letter.updated_at_ms = 1_000;
    upsert_stateful_dead_letter(&reliability_path, older_dead_letter)
        .await
        .expect("older dead letter");

    let first_page = get_json(
        state.clone(),
        format!("/stateful-runtime/runs/{run_id}/reliability?limit=1"),
        &tenant_a,
    )
    .await;
    assert_eq!(
        first_page["pagination"]["next"]["dead_letters"]["after_collection"],
        json!("dead_letters")
    );
    let after_id = first_page["pagination"]["next"]["dead_letters"]["after_id"]
        .as_str()
        .expect("dead-letter cursor")
        .to_string();
    assert_eq!(after_id, "dead-newer");

    let second_page = get_json(
        state.clone(),
        format!("/stateful-runtime/runs/{run_id}/reliability?limit=1&after_id={after_id}"),
        &tenant_a,
    )
    .await;
    assert_eq!(
        second_page["pagination"]["after_collection"],
        json!("dead_letters")
    );
    assert_eq!(second_page["counts"]["outbox"], json!(1));
    assert_eq!(second_page["counts"]["tool_effects"], json!(1));
    assert_eq!(second_page["counts"]["compensations"], json!(1));
    assert_eq!(second_page["counts"]["dead_letters"], json!(1));
    assert_eq!(
        second_page["dead_letters"][0]["dead_letter_id"],
        json!("dead-older")
    );

    let stale_page = get_json(
        state,
        format!("/stateful-runtime/runs/{run_id}/reliability?limit=1&after_id=dead-missing"),
        &tenant_a,
    )
    .await;
    assert_eq!(stale_page["counts"]["outbox"], json!(0));
    assert_eq!(stale_page["counts"]["tool_effects"], json!(0));
    assert_eq!(stale_page["counts"]["compensations"], json!(0));
    assert_eq!(stale_page["counts"]["dead_letters"], json!(0));
}

#[tokio::test]
async fn stateful_runtime_reliability_cursor_infers_anchor_beyond_first_page() {
    let state = test_state().await;
    let tenant_a = tenant("org-cursor-deep-a", "workspace-a", "operator-a");
    let run_id = "run-reliability-cursor-deep";
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let scope_a = scoped_runtime(&tenant_a, "finance", "repo-finance", "grant-finance");

    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), automation_run(run_id, tenant_a.clone()));
    let dead_letters = (0..1_005)
        .map(|index| {
            let mut row = dead_letter_record(&format!("dead-{index:04}"), run_id, scope_a.clone());
            row.created_at_ms = index as u64;
            row.updated_at_ms = index as u64;
            row
        })
        .collect::<Vec<_>>();
    let store = StatefulReliabilityStoreFile {
        schema_version: STATEFUL_RUNTIME_SCHEMA_VERSION,
        outbox: Vec::new(),
        tool_effects: Vec::new(),
        dead_letters,
        compensations: Vec::new(),
    };
    OrchestrationStateStore::from_runtime_events_path(&state.runtime_events_path)
        .expect("open authoritative runtime store")
        .upsert_stateful_runtime_reliability(&store)
        .expect("seed authoritative reliability store");

    let page = get_json(
        state,
        format!("/stateful-runtime/runs/{run_id}/reliability?limit=1&after_id=dead-0004"),
        &tenant_a,
    )
    .await;

    assert_eq!(
        page["pagination"]["after_collection"],
        json!("dead_letters")
    );
    assert_eq!(page["counts"]["dead_letters"], json!(1));
    assert_eq!(
        page["dead_letters"][0]["dead_letter_id"],
        json!("dead-0003")
    );
}

#[tokio::test]
async fn stateful_runtime_reliability_cursor_preserves_created_time_bound() {
    let state = test_state().await;
    let tenant_a = tenant("org-cursor-window-a", "workspace-a", "operator-a");
    let run_id = "run-reliability-cursor-window";
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let scope_a = scoped_runtime(&tenant_a, "finance", "repo-finance", "grant-finance");

    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), automation_run(run_id, tenant_a.clone()));
    let mut first_in_window = dead_letter_record("dead-window-first", run_id, scope_a.clone());
    first_in_window.created_at_ms = 3_000;
    first_in_window.updated_at_ms = 9_000;
    let mut outside_window = dead_letter_record("dead-outside-window", run_id, scope_a.clone());
    outside_window.created_at_ms = 5_000;
    outside_window.updated_at_ms = 8_500;
    let mut second_in_window = dead_letter_record("dead-window-second", run_id, scope_a);
    second_in_window.created_at_ms = 2_000;
    second_in_window.updated_at_ms = 8_000;
    for row in [first_in_window, outside_window, second_in_window] {
        upsert_stateful_dead_letter(&reliability_path, row)
            .await
            .expect("dead letter");
    }

    let first_page = get_json(
        state.clone(),
        format!("/stateful-runtime/runs/{run_id}/reliability?limit=1&before_created_at_ms=4000"),
        &tenant_a,
    )
    .await;
    assert_eq!(
        first_page["dead_letters"][0]["dead_letter_id"],
        json!("dead-window-first")
    );
    let cursor = &first_page["pagination"]["next"]["dead_letters"];
    assert_eq!(cursor["before_created_at_ms"], json!(4_000));
    let after_id = cursor["after_id"].as_str().expect("after id");
    let after_collection = cursor["after_collection"]
        .as_str()
        .expect("after collection");
    let before_created_at_ms = cursor["before_created_at_ms"]
        .as_u64()
        .expect("created-time bound");

    let second_page = get_json(
        state,
        format!(
            "/stateful-runtime/runs/{run_id}/reliability?limit=1&after_id={after_id}&after_collection={after_collection}&before_created_at_ms={before_created_at_ms}"
        ),
        &tenant_a,
    )
    .await;

    assert_eq!(second_page["counts"]["dead_letters"], json!(1));
    assert_eq!(
        second_page["dead_letters"][0]["dead_letter_id"],
        json!("dead-window-second")
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
    let mut superseded_dead_letter = dead_letter_record("dead-superseded", run_id, scope_a.clone());
    superseded_dead_letter.metadata = Some(json!({
        "superseded_by_success": true,
        "superseded_by_effect_id": "effect-sent",
        "superseded_at_ms": 3_000,
    }));
    upsert_stateful_dead_letter(&reliability_path, superseded_dead_letter)
        .await
        .expect("superseded dead letter");
    let mut superseded_compensation =
        compensation_record("comp-superseded", "effect-sent", run_id, scope_a.clone());
    superseded_compensation.metadata = Some(json!({
        "superseded_by_success": true,
        "superseded_by_effect_id": "effect-sent",
        "superseded_at_ms": 3_000,
    }));
    upsert_stateful_compensation(&reliability_path, superseded_compensation)
        .await
        .expect("superseded compensation");
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
    let operator_choices = plan["operator_choices"]
        .as_array()
        .expect("operator choices");
    assert!(operator_choices
        .iter()
        .any(|choice| choice["choice"] == "resume_from_checkpoint"));
    let retry_choice = operator_choices
        .iter()
        .find(|choice| choice["choice"] == "retry_failed_effect")
        .expect("retry choice");
    // TAN-564: a retry choice now re-drives the owning run through its governed
    // dispatch path instead of merely recording intent.
    assert_eq!(
        retry_choice["execution_mode"],
        json!("automatic_retry_dispatch")
    );
    assert_eq!(retry_choice["automatic_dispatch"], json!(true));
    let compensation_choice = operator_choices
        .iter()
        .find(|choice| choice["choice"] == "compensate_pending_effects")
        .expect("compensation choice");
    assert_eq!(
        compensation_choice["execution_mode"],
        json!("stateful_compensation_engine")
    );
    assert_eq!(compensation_choice["automatic_dispatch"], json!(true));
    assert_payload_excludes_hidden_runtime_rows(&plan);
    let body = plan.to_string();
    assert!(!body.contains("dead-superseded"), "{body}");
    assert!(!body.contains("comp-superseded"), "{body}");
}

#[tokio::test]
async fn stateful_runtime_resume_plan_filters_superseded_recovery_rows_before_limit() {
    let state = test_state().await;
    let tenant_a = tenant("org-recovery-a", "workspace-a", "operator-a");
    let run_id = "run-superseded-before-limit";
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let scope_a = scoped_runtime(&tenant_a, "finance", "repo-finance", "grant-finance");

    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), automation_run(run_id, tenant_a.clone()));

    let mut active_dead_letter = dead_letter_record("dead-active", run_id, scope_a.clone());
    active_dead_letter.created_at_ms = 1_000;
    active_dead_letter.updated_at_ms = 1_000;
    upsert_stateful_dead_letter(&reliability_path, active_dead_letter)
        .await
        .expect("active dead letter");
    let mut superseded_dead_letter =
        dead_letter_record("dead-superseded-newer", run_id, scope_a.clone());
    superseded_dead_letter.created_at_ms = 2_000;
    superseded_dead_letter.updated_at_ms = 2_000;
    superseded_dead_letter.metadata = Some(json!({
        "superseded_by_success": true,
        "superseded_by_effect_id": "effect-replayed",
        "superseded_at_ms": 3_000,
    }));
    upsert_stateful_dead_letter(&reliability_path, superseded_dead_letter)
        .await
        .expect("superseded dead letter");

    let mut active_compensation =
        compensation_record("comp-active", "effect-active", run_id, scope_a.clone());
    active_compensation.created_at_ms = 1_000;
    active_compensation.updated_at_ms = 1_000;
    upsert_stateful_compensation(&reliability_path, active_compensation)
        .await
        .expect("active compensation");
    let mut superseded_compensation =
        compensation_record("comp-superseded-newer", "effect-replayed", run_id, scope_a);
    superseded_compensation.created_at_ms = 2_000;
    superseded_compensation.updated_at_ms = 2_000;
    superseded_compensation.metadata = Some(json!({
        "superseded_by_success": true,
        "superseded_by_effect_id": "effect-replayed",
        "superseded_at_ms": 3_000,
    }));
    upsert_stateful_compensation(&reliability_path, superseded_compensation)
        .await
        .expect("superseded compensation");

    let plan = get_json(
        state,
        format!("/stateful-runtime/runs/{run_id}/resume-plan?limit=1"),
        &tenant_a,
    )
    .await;
    assert_eq!(plan["audit_summary"]["dead_letter_count"], json!(1));
    assert_eq!(
        plan["audit_summary"]["pending_compensation_count"],
        json!(1)
    );
    assert_eq!(plan["dead_letters"][0]["dead_letter_id"], "dead-active");
    assert_eq!(
        plan["pending_compensations"][0]["compensation_id"],
        "comp-active"
    );
    let body = plan.to_string();
    assert!(!body.contains("dead-superseded-newer"), "{body}");
    assert!(!body.contains("comp-superseded-newer"), "{body}");
}

#[tokio::test]
async fn stateful_runtime_reliability_active_recovery_filters_superseded_rows_before_limit() {
    let state = test_state().await;
    let tenant_a = tenant("org-reliability-active-a", "workspace-a", "operator-a");
    let run_id = "run-active-recovery-before-limit";
    let reliability_path =
        stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    let scope_a = scoped_runtime(&tenant_a, "finance", "repo-finance", "grant-finance");

    let mut active_dead_letter = dead_letter_record("dead-active", run_id, scope_a.clone());
    active_dead_letter.created_at_ms = 1_000;
    active_dead_letter.updated_at_ms = 1_000;
    upsert_stateful_dead_letter(&reliability_path, active_dead_letter)
        .await
        .expect("active dead letter");
    let mut superseded_dead_letter =
        dead_letter_record("dead-superseded-newer", run_id, scope_a.clone());
    superseded_dead_letter.created_at_ms = 2_000;
    superseded_dead_letter.updated_at_ms = 2_000;
    superseded_dead_letter.metadata = Some(json!({
        "superseded_by_success": true,
        "superseded_by_effect_id": "effect-replayed",
        "superseded_at_ms": 3_000,
    }));
    upsert_stateful_dead_letter(&reliability_path, superseded_dead_letter)
        .await
        .expect("superseded dead letter");

    let mut active_compensation =
        compensation_record("comp-active", "effect-active", run_id, scope_a.clone());
    active_compensation.created_at_ms = 1_000;
    active_compensation.updated_at_ms = 1_000;
    upsert_stateful_compensation(&reliability_path, active_compensation)
        .await
        .expect("active compensation");
    let mut superseded_compensation =
        compensation_record("comp-superseded-newer", "effect-replayed", run_id, scope_a);
    superseded_compensation.created_at_ms = 2_000;
    superseded_compensation.updated_at_ms = 2_000;
    superseded_compensation.metadata = Some(json!({
        "superseded_by_success": true,
        "superseded_by_effect_id": "effect-replayed",
        "superseded_at_ms": 3_000,
    }));
    upsert_stateful_compensation(&reliability_path, superseded_compensation)
        .await
        .expect("superseded compensation");

    let page = get_json(
        state,
        "/stateful-runtime/reliability?limit=1&active_recovery_only=true",
        &tenant_a,
    )
    .await;

    assert_eq!(
        page["dead_letters"][0]["dead_letter_id"],
        json!("dead-active")
    );
    assert_eq!(
        page["compensations"][0]["compensation_id"],
        json!("comp-active")
    );
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

async fn seed_runtime_delegation_grant(
    state: &AppState,
    tenant: &TenantContext,
    org_unit_id: &str,
    resource_id: &str,
    grant_id: &str,
) {
    let org_unit = OrganizationUnit::active(
        org_unit_id,
        tenant.clone(),
        format!("{org_unit_id} Ops"),
        OrganizationUnitKind::Department,
        PrincipalRef::human_user(tenant.actor_id.as_deref().unwrap_or("operator")),
        1,
    );
    let resource = ResourceRef::new(
        tenant.org_id.clone(),
        tenant.workspace_id.clone(),
        ResourceKind::Repository,
        resource_id,
    );
    let grant = OrganizationUnitAccessGrant::active(
        grant_id,
        tenant.clone(),
        org_unit.principal_ref(),
        resource,
        1,
    )
    .with_permissions(vec![AccessPermission::Read])
    .with_data_classes(vec![DataClass::FinancialRecord]);

    state
        .enterprise
        .org_units
        .write()
        .await
        .insert(org_unit_id.to_string(), org_unit);
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert(grant_id.to_string(), grant);
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
        requester_context: None,
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

// TAN-566 — lost durable-wait wake recovery on restart.

fn paused_run(
    run_id: &str,
    tenant_context: TenantContext,
    updated_at_ms: u64,
) -> AutomationV2RunRecord {
    let mut run = automation_run(run_id, tenant_context);
    run.status = AutomationRunStatus::Paused;
    run.pause_reason = Some("timer wait".to_string());
    run.updated_at_ms = updated_at_ms;
    run
}

fn woken_wait(
    wait_id: &str,
    run_id: &str,
    scope: StatefulRuntimeScope,
    updated_at_ms: u64,
) -> StatefulWaitRecord {
    let mut wait = wait_record(wait_id, run_id, scope);
    wait.wait_kind = StatefulWaitKind::Timer;
    wait.status = StatefulWaitStatus::Woken;
    wait.event_seq = Some(7);
    wait.wake_idempotency_key = Some(format!("wake:{wait_id}"));
    wait.updated_at_ms = updated_at_ms;
    wait
}

#[tokio::test]
async fn lost_stateful_wait_wake_requeues_paused_run_on_restart() {
    let state = test_state().await;
    let tenant = tenant("org-wake-recovery", "workspace-a", "operator-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    let run_id = "run-lost-wake";
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);

    state.automation_v2_runs.write().await.insert(
        run_id.to_string(),
        paused_run(run_id, tenant.clone(), 2_000),
    );
    // The wake fired (seq set) at/after the run's last state change, then the
    // in-memory requeue was lost to a crash.
    upsert_stateful_wait(
        &paths.waits_path,
        woken_wait("wait-woken", run_id, scope, 2_500),
    )
    .await
    .expect("seed woken wait");

    let recovered = state.recover_in_flight_runs().await;
    assert_eq!(recovered, 1, "the lost wake should recover exactly one run");

    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run still present");
    assert_eq!(
        run.status,
        AutomationRunStatus::Queued,
        "a run whose durable wake was lost must be requeued on restart"
    );
    assert_eq!(
        run.resume_reason.as_deref(),
        Some("stateful_wait_wake_recovered_on_restart")
    );
    assert!(run.pause_reason.is_none());
}

#[tokio::test]
async fn paused_run_with_active_wait_is_not_requeued() {
    // Legitimately parked on a live wait (Waiting) — must be left for the
    // scheduler even if an older Woken wait also exists for the run.
    let state = test_state().await;
    let tenant = tenant("org-wake-recovery", "workspace-b", "operator-b");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    let run_id = "run-active-wait";
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);

    state.automation_v2_runs.write().await.insert(
        run_id.to_string(),
        paused_run(run_id, tenant.clone(), 2_000),
    );
    upsert_stateful_wait(
        &paths.waits_path,
        woken_wait("wait-old", run_id, scope.clone(), 2_100),
    )
    .await
    .expect("seed old woken wait");
    // wait_record defaults to StatefulWaitStatus::Waiting — an active wait.
    upsert_stateful_wait(&paths.waits_path, wait_record("wait-active", run_id, scope))
        .await
        .expect("seed active wait");

    let recovered = state.recover_in_flight_runs().await;
    assert_eq!(recovered, 0);
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(run.status, AutomationRunStatus::Paused);
}

#[tokio::test]
async fn run_paused_after_its_wake_is_not_requeued() {
    // The wake fired *before* the run's last state change (e.g. a manual
    // re-pause), so wait.updated_at_ms < run.updated_at_ms — not a lost wake.
    let state = test_state().await;
    let tenant = tenant("org-wake-recovery", "workspace-c", "operator-c");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    let run_id = "run-repaused";
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);

    state.automation_v2_runs.write().await.insert(
        run_id.to_string(),
        paused_run(run_id, tenant.clone(), 3_000),
    );
    upsert_stateful_wait(
        &paths.waits_path,
        woken_wait("wait-stale", run_id, scope, 2_000),
    )
    .await
    .expect("seed stale woken wait");

    let recovered = state.recover_in_flight_runs().await;
    assert_eq!(recovered, 0);
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(run.status, AutomationRunStatus::Paused);
}

#[tokio::test]
async fn foreign_tenant_woken_wait_does_not_requeue_run() {
    // The waits store is shared and a run_id can collide across tenants. A
    // `Woken` wait belonging to another tenant must not requeue this tenant's
    // paused run (cross-tenant false positive).
    let state = test_state().await;
    let tenant_a = tenant("org-wake-a", "workspace-a", "operator-a");
    let tenant_b = tenant("org-wake-b", "workspace-b", "operator-b");
    let run_id = "run-shared-id";
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);

    state.automation_v2_runs.write().await.insert(
        run_id.to_string(),
        paused_run(run_id, tenant_a.clone(), 2_000),
    );
    let foreign_scope = StatefulRuntimeScope::from_tenant_context(tenant_b);
    upsert_stateful_wait(
        &paths.waits_path,
        woken_wait("wait-foreign", run_id, foreign_scope, 2_500),
    )
    .await
    .expect("seed foreign woken wait");

    let recovered = state.recover_in_flight_runs().await;
    assert_eq!(recovered, 0);
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(run.status, AutomationRunStatus::Paused);
}

#[tokio::test]
async fn foreign_active_wait_does_not_block_own_lost_wake_recovery() {
    // A foreign tenant's active (`Waiting`) wait sharing this run_id must not
    // make this tenant's genuinely-lost wake unrecoverable (false negative).
    let state = test_state().await;
    let tenant_a = tenant("org-wake-a", "workspace-a", "operator-a");
    let tenant_b = tenant("org-wake-b", "workspace-b", "operator-b");
    let run_id = "run-shared-id-2";
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);

    state.automation_v2_runs.write().await.insert(
        run_id.to_string(),
        paused_run(run_id, tenant_a.clone(), 2_000),
    );
    // Foreign active wait (tenant B) — must be ignored for tenant A's run.
    upsert_stateful_wait(
        &paths.waits_path,
        wait_record(
            "wait-foreign-active",
            run_id,
            StatefulRuntimeScope::from_tenant_context(tenant_b),
        ),
    )
    .await
    .expect("seed foreign active wait");
    // This tenant's own lost wake.
    upsert_stateful_wait(
        &paths.waits_path,
        woken_wait(
            "wait-own",
            run_id,
            StatefulRuntimeScope::from_tenant_context(tenant_a),
            2_500,
        ),
    )
    .await
    .expect("seed own woken wait");

    let recovered = state.recover_in_flight_runs().await;
    assert_eq!(recovered, 1);
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(run.status, AutomationRunStatus::Queued);
}

// TAN-564 — dead-letter retry actually re-executes the failed effect.

/// A `Failed` run wired to a stored automation snapshot whose flow has a
/// `review` node (→ `approval` descendant), used to exercise the dead-letter
/// retry checkpoint reset.
async fn failed_run_with_snapshot(
    state: &AppState,
    run_id: &str,
    tenant: TenantContext,
    blocked_node: &str,
) -> AutomationV2RunRecord {
    let spec =
        crate::http::tests::global::create_test_automation_v2(state, "automation-dead-letter")
            .await;
    let mut run = automation_run(run_id, tenant);
    run.automation_id = "automation-dead-letter".to_string();
    run.automation_snapshot = Some(spec);
    run.status = AutomationRunStatus::Failed;
    run.checkpoint.completed_nodes = vec!["draft".to_string()];
    run.checkpoint.blocked_nodes = vec![blocked_node.to_string()];
    run.checkpoint.pending_nodes = Vec::new();
    run
}

fn retry_requested_dead_letter(
    dead_letter_id: &str,
    run_id: &str,
    scope: StatefulRuntimeScope,
) -> StatefulDeadLetterRecord {
    let mut record = dead_letter_record(dead_letter_id, run_id, scope);
    record.status = StatefulDeadLetterStatus::RetryRequested;
    record
}

async fn read_dead_letter(
    state: &AppState,
    tenant: &TenantContext,
    run_id: &str,
    dead_letter_id: &str,
) -> StatefulDeadLetterRecord {
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);
    list_stateful_dead_letters(
        &path,
        tenant,
        StatefulReliabilityQuery {
            run_id: Some(run_id),
            status: None,
            source_type: None,
            after_id: None,
            before_created_at_ms: None,
            active_recovery_only: false,
            limit: Some(50),
        },
    )
    .into_iter()
    .find(|row| row.dead_letter_id == dead_letter_id)
    .unwrap_or_else(|| panic!("dead letter {dead_letter_id} present"))
}

#[tokio::test]
async fn retry_requested_dead_letter_requeues_owning_run() {
    let state = test_state().await;
    let tenant = tenant("org-dead-letter", "workspace-a", "operator-a");
    let scope = StatefulRuntimeScope::from_tenant_context(tenant.clone());
    let run_id = "run-dead-letter-retry";
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);

    let run = failed_run_with_snapshot(&state, run_id, tenant.clone(), "review").await;
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), run);
    upsert_stateful_dead_letter(
        &path,
        retry_requested_dead_letter("dead-review-effect", run_id, scope),
    )
    .await
    .expect("seed retry-requested dead letter");

    let acted = state.dispatch_ready_stateful_dead_letter_retries().await;
    assert_eq!(
        acted, 1,
        "the retry should dispatch exactly one dead letter"
    );

    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(
        run.status,
        AutomationRunStatus::Queued,
        "a requested retry must re-queue the owning run so the effect re-executes"
    );
    assert_eq!(
        run.resume_reason.as_deref(),
        Some("stateful_dead_letter_retry_dispatched")
    );
    assert!(
        !run.checkpoint.blocked_nodes.contains(&"review".to_string()),
        "the failed node must be cleared from blocked_nodes so it can re-run"
    );
    assert!(
        run.checkpoint.pending_nodes.contains(&"review".to_string()),
        "the failed node must be re-queued as pending"
    );
    // The descendant of the failed node is reset too (must re-run after it).
    assert!(run
        .checkpoint
        .pending_nodes
        .contains(&"approval".to_string()));

    let dead_letter = read_dead_letter(&state, &tenant, run_id, "dead-review-effect").await;
    assert_eq!(dead_letter.status, StatefulDeadLetterStatus::Retrying);
    // The node/tool `attempts` field is untouched — the dispatcher tracks its
    // own retry count separately so a dead letter born on a high node attempt
    // is not pre-exhausted.
    assert_eq!(dead_letter.attempts, 2);
    assert_eq!(
        dead_letter
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("retry_dispatch_count"))
            .and_then(|value| value.as_u64()),
        Some(1),
        "the dispatch must increment the dispatcher retry count"
    );
    assert!(
        dead_letter
            .metadata
            .as_ref()
            .and_then(|meta| meta.get("retry_dispatched_at_ms"))
            .is_some(),
        "the dispatch time must be stamped for backoff"
    );
}

#[tokio::test]
async fn foreign_tenant_dead_letter_does_not_requeue_run() {
    // The reliability store is shared across tenants; a foreign-tenant dead
    // letter that happens to carry this run's id must never drive its recovery.
    let state = test_state().await;
    let tenant_a = tenant("org-dl-a", "workspace-a", "operator-a");
    let tenant_b = tenant("org-dl-b", "workspace-b", "operator-b");
    let run_id = "run-dead-letter-foreign";
    let path = stateful_reliability_path_from_runtime_events_path(&state.runtime_events_path);

    let run = failed_run_with_snapshot(&state, run_id, tenant_a.clone(), "review").await;
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run_id.to_string(), run);
    upsert_stateful_dead_letter(
        &path,
        retry_requested_dead_letter(
            "dead-foreign-effect",
            run_id,
            StatefulRuntimeScope::from_tenant_context(tenant_b),
        ),
    )
    .await
    .expect("seed foreign dead letter");

    let acted = state.dispatch_ready_stateful_dead_letter_retries().await;
    assert_eq!(
        acted, 0,
        "a foreign-tenant dead letter must not be acted on"
    );

    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run present");
    assert_eq!(
        run.status,
        AutomationRunStatus::Failed,
        "the run must stay failed when only a foreign dead letter requests retry"
    );
}

#[path = "stateful_runtime_hardening_dead_letter_retry.rs"]
mod stateful_runtime_hardening_dead_letter_retry;
