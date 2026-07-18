// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

use tandem_types::{
    AuthorityChain, HumanActor, MessagePartInput, ModelSpec, RequestPrincipal, SendMessageRequest,
    Session, ToolRiskTier, VerifiedTenantContext,
};

fn minimal_automation(
    automation_id: &str,
    tenant_context: &TenantContext,
) -> crate::AutomationV2Spec {
    let mut automation = crate::AutomationV2Spec {
        automation_id: automation_id.to_string(),
        name: "Operator test automation".to_string(),
        description: None,
        status: crate::AutomationV2Status::Draft,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy::default(),
        output_targets: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        creator_id: "operator-test".to_string(),
        workspace_root: None,
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    automation.set_tenant_context(tenant_context);
    automation
}

fn verified_context(
    tenant_context: TenantContext,
    actor_id: &str,
    roles: Vec<String>,
    capabilities: Vec<String>,
) -> VerifiedTenantContext {
    let principal = RequestPrincipal::authenticated_user(actor_id, "operator-test");
    VerifiedTenantContext {
        tenant_context,
        human_actor: HumanActor::tandem_user(actor_id),
        authority_chain: AuthorityChain::from_request(principal),
        roles,
        org_units: Vec::new(),
        capabilities,
        policy_version: None,
        strict_projection: None,
        issuer: "operator-test".to_string(),
        audience: "tandem".to_string(),
        issued_at_ms: crate::now_ms(),
        expires_at_ms: crate::now_ms() + 60_000,
        assertion_id: format!("operator-test-{actor_id}"),
        assertion_key_id: None,
    }
}

async fn chat_session(
    state: &AppState,
    tenant_context: TenantContext,
    verified: Option<VerifiedTenantContext>,
) -> Session {
    let mut session = Session::new(
        Some("Operator tool test".to_string()),
        Some("/tmp/operator-tests".to_string()),
    );
    session.tenant_context = tenant_context;
    session.verified_tenant_context = verified;
    state
        .storage
        .save_session(session.clone())
        .await
        .expect("save chat session");
    session
}

fn operator_tool(state: AppState, name: &str) -> std::sync::Arc<dyn tandem_tools::Tool> {
    crate::http::operator_tools::operator_tools(state)
        .into_iter()
        .find(|tool| tool.schema().name == name)
        .unwrap_or_else(|| panic!("missing operator tool {name}"))
}

fn planner_record(
    tenant_context: TenantContext,
    planner_session_id: &str,
    chat_session_id: &str,
    updated_at_ms: u64,
) -> crate::http::workflow_planner::WorkflowPlannerSessionRecord {
    crate::http::workflow_planner::WorkflowPlannerSessionRecord {
        session_id: planner_session_id.to_string(),
        tenant_context,
        linked_chat_session_id: Some(chat_session_id.to_string()),
        linked_chat_run_id: Some(format!("run-{planner_session_id}")),
        last_referenced_at_ms: None,
        artifact_links: Vec::new(),
        project_slug: "operator-tests".to_string(),
        title: format!("Plan {planner_session_id}"),
        workspace_root: "/tmp/operator-tests".to_string(),
        source_kind: "agentic_chat".to_string(),
        source_bundle_digest: None,
        source_pack_id: None,
        source_pack_version: None,
        current_plan_id: Some(format!("plan-{planner_session_id}")),
        draft: None,
        goal: "Build a workflow".to_string(),
        notes: String::new(),
        planner_provider: String::new(),
        planner_model: String::new(),
        plan_source: "agentic_chat".to_string(),
        allowed_mcp_servers: Vec::new(),
        operator_preferences: None,
        planning: None,
        import_validation: None,
        import_transform_log: Vec::new(),
        import_scope_snapshot: None,
        operation: None,
        published_at_ms: None,
        published_tasks: Vec::new(),
        created_at_ms: updated_at_ms,
        updated_at_ms,
    }
}

#[tokio::test]
async fn operator_tool_catalog_separates_reads_drafts_and_consequential_controls() {
    let tools = crate::http::operator_tools::operator_tools(test_state().await);
    let schemas = tools.iter().map(|tool| tool.schema()).collect::<Vec<_>>();
    assert_eq!(
        schemas
            .iter()
            .map(|schema| schema.name.as_str())
            .collect::<Vec<_>>(),
        vec![
            "workflow_plan_start",
            "workflow_plan_read",
            "workflow_plan_revise",
            "workflow_plan_preview",
            "workflow_plan_validate",
            "workflow_plan_materialize",
            "automation_inspect",
            "automation_manage_draft",
            "automation_control",
            "orchestration_inspect",
            "workflow_plan_capabilities",
        ]
    );
    for name in [
        "workflow_plan_read",
        "workflow_plan_preview",
        "workflow_plan_validate",
        "automation_inspect",
        "orchestration_inspect",
        "workflow_plan_capabilities",
    ] {
        let schema = schemas.iter().find(|schema| schema.name == name).unwrap();
        assert_eq!(schema.security.risk_tier, Some(ToolRiskTier::ReadDiscover));
    }
    let control = schemas
        .iter()
        .find(|schema| schema.name == "automation_control")
        .unwrap();
    assert_eq!(
        control.security.risk_tier,
        Some(ToolRiskTier::ConsequentialWrite)
    );

    let workflow_start = schemas
        .iter()
        .find(|schema| schema.name == "workflow_plan_start")
        .unwrap();
    assert!(workflow_start
        .description
        .contains("Required first step for creating a new workflow"));

    let automation_draft = schemas
        .iter()
        .find(|schema| schema.name == "automation_manage_draft")
        .unwrap();
    assert!(automation_draft
        .description
        .contains("for new natural-language creation use workflow_plan_start"));
    let actions = automation_draft.input_schema["properties"]["action"]["enum"]
        .as_array()
        .unwrap();
    assert!(!actions
        .iter()
        .any(|action| action.as_str() == Some("create")));
}

#[tokio::test]
async fn operator_artifact_context_is_tenant_scoped_and_refuses_ambiguous_followups() {
    let state = test_state().await;
    let tenant_a = TenantContext::explicit("org-a", "workspace-a", None);
    let tenant_b = TenantContext::explicit("org-b", "workspace-b", None);
    state
        .put_workflow_planner_session(planner_record(tenant_a.clone(), "planner-a1", "chat-1", 10))
        .await
        .unwrap();
    let single =
        crate::http::operator_tools_context::operator_artifact_context(&state, &tenant_a, "chat-1")
            .await;
    assert_eq!(single["selection"], json!("single_active"));
    assert_eq!(
        single.pointer("/active/planner_session_id"),
        Some(&json!("planner-a1"))
    );
    assert_eq!(
        single.pointer("/active/url"),
        Some(&json!("/#/planner?session_id=planner-a1"))
    );

    state
        .put_workflow_planner_session(planner_record(tenant_a.clone(), "planner-a2", "chat-1", 20))
        .await
        .unwrap();
    state
        .put_workflow_planner_session(planner_record(tenant_b.clone(), "planner-b1", "chat-1", 30))
        .await
        .unwrap();
    let ambiguous =
        crate::http::operator_tools_context::operator_artifact_context(&state, &tenant_a, "chat-1")
            .await;
    assert_eq!(ambiguous["selection"], json!("ambiguous"));
    assert!(ambiguous["active"].is_null());
    assert_eq!(ambiguous["recent"].as_array().unwrap().len(), 2);

    let mut selected = planner_record(tenant_a.clone(), "planner-a2", "chat-1", 20);
    selected.last_referenced_at_ms = Some(40);
    state.put_workflow_planner_session(selected).await.unwrap();
    let resolved =
        crate::http::operator_tools_context::operator_artifact_context(&state, &tenant_a, "chat-1")
            .await;
    assert_eq!(resolved["selection"], json!("single_active"));
    assert_eq!(
        resolved.pointer("/active/planner_session_id"),
        Some(&json!("planner-a2"))
    );

    let foreign =
        crate::http::operator_tools_context::operator_artifact_context(&state, &tenant_b, "chat-1")
            .await;
    assert_eq!(foreign["selection"], json!("single_active"));
    assert_eq!(foreign["recent"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn prompt_submission_idempotency_replays_the_original_durable_run() {
    let state = test_state().await;
    let tenant = TenantContext::local_implicit();
    let request: SendMessageRequest = serde_json::from_value(json!({
        "parts": [{ "type": "text", "text": "Create a daily report workflow" }]
    }))
    .unwrap_or_else(|_| SendMessageRequest {
        parts: vec![MessagePartInput::Text {
            text: "Create a daily report workflow".to_string(),
        }],
        model: None,
        agent: None,
        tool_mode: None,
        tool_allowlist: None,
        strict_kb_grounding: None,
        context_mode: None,
        write_required: None,
        prewrite_requirements: None,
        sampling: Default::default(),
    });
    let mut headers = HeaderMap::new();
    headers.insert(
        "idempotency-key",
        HeaderValue::from_static("prompt-replay-1"),
    );

    let first = crate::http::session_run_idempotency::reserve_prompt_submission(
        &state,
        &tenant,
        "session-1",
        &headers,
        &request,
    )
    .await
    .unwrap()
    .unwrap();
    let crate::http::session_run_idempotency::PromptSubmissionDecision::Reserved(reservation) =
        first
    else {
        panic!("first submission should reserve");
    };
    crate::http::session_run_idempotency::complete_prompt_submission(
        &state,
        &tenant,
        &reservation,
        "session-1",
        "run-1",
        "session-session-1",
    )
    .await
    .unwrap();

    let replay = crate::http::session_run_idempotency::reserve_prompt_submission(
        &state,
        &tenant,
        "session-1",
        &headers,
        &request,
    )
    .await
    .unwrap()
    .unwrap();
    let crate::http::session_run_idempotency::PromptSubmissionDecision::Replay(payload) = replay
    else {
        panic!("duplicate submission should replay");
    };
    assert_eq!(payload["runID"], json!("run-1"));
    assert_eq!(payload["contextRunID"], json!("session-session-1"));
    assert_eq!(payload["idempotentReplay"], json!(true));
}

#[tokio::test]
async fn workflow_planner_session_http_reads_are_tenant_scoped() {
    let state = test_state().await;
    let tenant_a = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "actor-a");
    state
        .put_workflow_planner_session(planner_record(
            tenant_a,
            "planner-private",
            "chat-private",
            10,
        ))
        .await
        .unwrap();
    let app = app_router(state);

    let foreign = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/workflow-plans/sessions/planner-private")
                .header("x-tandem-org-id", "org-b")
                .header("x-tandem-workspace-id", "workspace-b")
                .header("x-tandem-actor-id", "actor-b")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(foreign.status(), StatusCode::NOT_FOUND);

    let owner = app
        .oneshot(
            Request::builder()
                .uri("/workflow-plans/sessions/planner-private")
                .header("x-tandem-org-id", "org-a")
                .header("x-tandem-workspace-id", "workspace-a")
                .header("x-tandem-actor-id", "actor-a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(owner.status(), StatusCode::OK);
}

#[tokio::test]
async fn operator_tools_reject_model_supplied_chat_session_substitution() {
    let state = test_state().await;
    let tool = operator_tool(state, "workflow_plan_capabilities");
    let error = tool
        .execute_for_tenant(
            json!({
                "chat_session_id": "model-selected-session",
                "__dispatch_session_id": "authenticated-session"
            }),
            TenantContext::local_implicit(),
        )
        .await
        .expect_err("session substitution must fail");
    assert!(error
        .to_string()
        .contains("must match the authenticated dispatch session"));
}

#[tokio::test]
async fn automation_draft_rejects_raw_creation_before_dispatch() {
    let state = test_state().await;
    let foreign_tenant =
        TenantContext::explicit_user_workspace("org-b", "workspace-b", None, "actor-b");
    let foreign = minimal_automation("shared-automation-id", &foreign_tenant);
    state.put_automation_v2(foreign.clone()).await.unwrap();
    let session = chat_session(&state, TenantContext::local_implicit(), None).await;
    let tool = operator_tool(state.clone(), "automation_manage_draft");

    let error = tool
        .execute_for_tenant(
            json!({
                "chat_session_id": session.id,
                "__dispatch_session_id": session.id,
                "action": "create",
                "automation": foreign,
                "idempotency_key": "foreign-id-collision"
            }),
            TenantContext::local_implicit(),
        )
        .await
        .expect_err("raw draft creation must fail");
    assert!(error
        .to_string()
        .contains("use workflow_plan_start and workflow_plan_materialize"));
    let stored = state
        .get_automation_v2("shared-automation-id")
        .await
        .expect("foreign automation remains");
    assert_eq!(stored.tenant_context().org_id, "org-b");
    assert_eq!(stored.creator_id, "operator-test");
}

#[tokio::test]
async fn hosted_automation_control_requires_operator_authority() {
    let state = test_state().await;
    let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "member-a");
    let verified = verified_context(tenant.clone(), "member-a", Vec::new(), Vec::new());
    let session = chat_session(&state, tenant.clone(), Some(verified.clone())).await;
    let tool = operator_tool(state, "automation_control");

    let error = tool
        .execute_for_tenant(
            json!({
                "chat_session_id": session.id,
                "__dispatch_session_id": session.id,
                "__verified_tenant_context": verified,
                "action": "disable",
                "automation_id": "automation-a",
                "idempotency_key": "control-without-role"
            }),
            tenant,
        )
        .await
        .expect_err("ordinary members cannot control automations");
    assert!(error
        .to_string()
        .contains("requires an operator role or automation-control capability"));
}

#[tokio::test]
async fn duplicate_workflow_start_keeps_the_original_reservation_in_progress() {
    let state = test_state().await;
    let tenant = TenantContext::local_implicit();
    let session = chat_session(&state, tenant.clone(), None).await;
    let args = json!({
        "__dispatch_session_id": session.id,
        "prompt": "Create a daily report workflow",
        "idempotency_key": "planner-start-in-flight"
    });
    let mut normalized = args.clone();
    normalized
        .as_object_mut()
        .unwrap()
        .retain(|key, _| !key.starts_with("__"));
    let fingerprint = crate::sha256_hex(&["operator.workflow_plan_start", &normalized.to_string()]);
    state
        .reserve_idempotency_key(crate::app::state::IdempotencyReservationInput {
            tenant_context: tenant.clone(),
            operation: "operator.workflow_plan_start".to_string(),
            key: "planner-start-in-flight".to_string(),
            owner: "local-operator".to_string(),
            request_fingerprint: fingerprint.clone(),
            first_seen_event_id: None,
            now_ms: crate::now_ms(),
            expires_at_ms: None,
        })
        .await
        .unwrap();

    let result = operator_tool(state.clone(), "workflow_plan_start")
        .execute_for_tenant(args, tenant.clone())
        .await
        .expect("duplicate call returns in-progress state");
    assert_eq!(result.metadata["status"], json!("in_progress"));
    let record = state
        .get_idempotency_key(
            &tenant,
            "operator.workflow_plan_start",
            "planner-start-in-flight",
        )
        .await
        .expect("reservation remains owned by original call");
    assert_eq!(record.request_fingerprint, fingerprint);
    assert!(record.outcome.is_none());
}

#[tokio::test]
async fn workflow_start_inherits_chat_model_and_prompt_schedule() {
    let state = test_state().await;
    let tenant = TenantContext::local_implicit();
    let mut session = chat_session(&state, tenant.clone(), None).await;
    session.model = Some(ModelSpec {
        provider_id: "missing-test-provider".to_string(),
        model_id: "planner-test-model".to_string(),
    });
    state
        .storage
        .save_session(session.clone())
        .await
        .expect("save modeled chat session");

    let result = operator_tool(state.clone(), "workflow_plan_start")
        .execute_for_tenant(
            json!({
                "__dispatch_session_id": session.id,
                "prompt": "Create a disabled support triage automation for weekdays at 08:00",
                "idempotency_key": "planner-start-inherits-chat-model"
            }),
            tenant,
        )
        .await
        .expect("workflow planner fallback should complete");
    let planner_session_id = result.metadata["resource"]["id"]
        .as_str()
        .expect("planner session id");
    let stored = state
        .get_workflow_planner_session(planner_session_id)
        .await
        .expect("stored planner session");

    assert_eq!(stored.planner_provider, "missing-test-provider");
    assert_eq!(stored.planner_model, "planner-test-model");
    assert_eq!(
        stored
            .operator_preferences
            .as_ref()
            .and_then(|value| value.pointer("/role_models/planner/provider_id"))
            .and_then(Value::as_str),
        Some("missing-test-provider")
    );
    let plan = &stored.draft.as_ref().expect("planner draft").current_plan;
    assert_eq!(
        plan.schedule.cron_expression.as_deref(),
        Some("0 8 * * 1-5")
    );
}
