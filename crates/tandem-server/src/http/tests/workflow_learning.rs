use super::*;

fn current_test_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64
}

fn sample_candidate(
    candidate_id: &str,
    workflow_id: &str,
    kind: crate::WorkflowLearningCandidateKind,
    status: crate::WorkflowLearningCandidateStatus,
) -> crate::WorkflowLearningCandidate {
    let now = current_test_ms();
    crate::WorkflowLearningCandidate {
        candidate_id: candidate_id.to_string(),
        workflow_id: workflow_id.to_string(),
        project_id: "proj-1".to_string(),
        source_run_id: format!("run-{candidate_id}"),
        kind,
        status,
        confidence: 0.9,
        summary: format!("summary for {candidate_id}"),
        fingerprint: format!("fingerprint-{candidate_id}"),
        node_id: Some("node-1".to_string()),
        node_kind: Some("report_markdown".to_string()),
        validator_family: Some("research_brief".to_string()),
        evidence_refs: vec![json!({"candidate_id": candidate_id})],
        artifact_refs: vec![format!("artifact://{candidate_id}/report.md")],
        proposed_memory_payload: Some(json!({
            "content": format!("memory for {candidate_id}")
        })),
        proposed_revision_prompt: Some(format!("Revise workflow using {candidate_id}")),
        source_memory_id: None,
        promoted_memory_id: None,
        needs_plan_bundle: false,
        baseline_before: None,
        latest_observed_metrics: None,
        last_revision_session_id: None,
        run_ids: vec![format!("run-{candidate_id}")],
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn sample_automation(workspace_root: &str, automation_id: &str) -> crate::AutomationV2Spec {
    crate::AutomationV2Spec {
        automation_id: automation_id.to_string(),
        name: format!("Workflow {automation_id}"),
        description: Some("workflow learning test automation".to_string()),
        status: crate::AutomationV2Status::Draft,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::Skip,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![crate::AutomationAgentProfile {
            agent_id: "agent-1".to_string(),
            template_id: None,
            display_name: "Worker".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: Vec::new(),
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![crate::AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "node-1".to_string(),
                agent_id: "agent-1".to_string(),
                objective: "Write a concise report".to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(crate::AutomationFlowOutputContract {
                    kind: "report".to_string(),
                    validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
                    enforcement: None,
                    schema: None,
                    summary_guidance: Some("Summarize the report.".to_string()),
                }),
                retry_policy: Some(json!({"max_attempts": 1})),
                timeout_ms: Some(60_000),
                max_tool_calls: None,
                stage_kind: None,
                gate: None,
                metadata: None,
            }],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: None,
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(workspace_root.to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

fn sample_plan_package_bundle() -> tandem_plan_compiler::api::PlanPackageImportBundle {
    let plan = tandem_plan_compiler::api::WorkflowPlanJson {
        plan_id: "plan_workflow_learning".to_string(),
        planner_version: "planner_v1".to_string(),
        plan_source: "test".to_string(),
        original_prompt: "Build a workflow.".to_string(),
        normalized_prompt: "Build a workflow.".to_string(),
        confidence: "medium".to_string(),
        title: "Workflow Learning Test".to_string(),
        description: None,
        schedule: tandem_plan_compiler::api::default_fallback_schedule_json(),
        execution_target: "automation_v2".to_string(),
        workspace_root: "/workspace".to_string(),
        steps: vec![tandem_plan_compiler::api::default_fallback_step_json()],
        requires_integrations: Vec::new(),
        allowed_mcp_servers: Vec::new(),
        operator_preferences: Some(json!({})),
        save_options: json!({}),
    };
    let package =
        tandem_plan_compiler::api::compile_workflow_plan_preview_package(&plan, Some("tester"));
    let exported = tandem_plan_compiler::api::export_plan_package_bundle(&package);
    tandem_plan_compiler::api::PlanPackageImportBundle {
        bundle_version: exported.bundle_version,
        plan: exported.plan,
        scope_snapshot: Some(exported.scope_snapshot),
    }
}

#[tokio::test]
async fn workflow_learning_candidates_list_filters_and_rejects_invalid_status() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let mut approved = sample_candidate(
        "wflearn-approved",
        "workflow-a",
        crate::WorkflowLearningCandidateKind::MemoryFact,
        crate::WorkflowLearningCandidateStatus::Approved,
    );
    approved.updated_at_ms = 20;
    let rejected = sample_candidate(
        "wflearn-rejected",
        "workflow-b",
        crate::WorkflowLearningCandidateKind::PromptPatch,
        crate::WorkflowLearningCandidateStatus::Rejected,
    );
    state
        .put_workflow_learning_candidate(approved)
        .await
        .expect("put approved candidate");
    state
        .put_workflow_learning_candidate(rejected)
        .await
        .expect("put rejected candidate");

    let req = Request::builder()
        .method("GET")
        .uri(
            "/workflow-learning/candidates?workflow_id=workflow-a&status=approved&kind=memory_fact",
        )
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("response body"),
    )
    .expect("response json");
    let candidates = payload
        .get("candidates")
        .and_then(Value::as_array)
        .cloned()
        .expect("candidate array");
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].get("candidate_id").and_then(Value::as_str),
        Some("wflearn-approved")
    );
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));

    let invalid_req = Request::builder()
        .method("GET")
        .uri("/workflow-learning/candidates?status=not-a-real-status")
        .body(Body::empty())
        .expect("invalid request");
    let invalid_resp = app
        .oneshot(invalid_req)
        .await
        .expect("invalid filter response");
    assert_eq!(invalid_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workflow_learning_candidate_review_updates_status_and_missing_candidate_is_404() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let candidate = sample_candidate(
        "wflearn-review",
        "workflow-review",
        crate::WorkflowLearningCandidateKind::PromptPatch,
        crate::WorkflowLearningCandidateStatus::Proposed,
    );
    state
        .put_workflow_learning_candidate(candidate)
        .await
        .expect("put review candidate");

    let review_req = Request::builder()
        .method("POST")
        .uri("/workflow-learning/candidates/wflearn-review/review")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "approve",
                "reviewer_id": "reviewer-1",
                "note": "approved for rollout"
            })
            .to_string(),
        ))
        .expect("review request");
    let review_resp = app
        .clone()
        .oneshot(review_req)
        .await
        .expect("review response");
    assert_eq!(review_resp.status(), StatusCode::OK);
    let review_payload: Value = serde_json::from_slice(
        &to_bytes(review_resp.into_body(), usize::MAX)
            .await
            .expect("review body"),
    )
    .expect("review json");
    assert_eq!(
        review_payload
            .get("candidate")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("approved")
    );
    let stored = state
        .get_workflow_learning_candidate("wflearn-review")
        .await
        .expect("stored candidate");
    assert_eq!(
        stored.status,
        crate::WorkflowLearningCandidateStatus::Approved
    );
    assert!(stored.evidence_refs.iter().any(|row| row
        .get("review_note")
        .and_then(Value::as_str)
        .is_some_and(|note| note == "approved for rollout")));

    let missing_req = Request::builder()
        .method("POST")
        .uri("/workflow-learning/candidates/missing-candidate/review")
        .header("content-type", "application/json")
        .body(Body::from(json!({"action": "approve"}).to_string()))
        .expect("missing request");
    let missing_resp = app
        .oneshot(missing_req)
        .await
        .expect("missing candidate response");
    assert_eq!(missing_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workflow_learning_candidate_promote_promotes_memory_fact_candidate() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "wflearn-promote-run",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "promote this learning",
                "classification": "internal",
                "artifact_refs": ["artifact://wflearn-promote/report.md"]
            })
            .to_string(),
        ))
        .expect("memory put request");
    let put_resp = app
        .clone()
        .oneshot(put_req)
        .await
        .expect("memory put response");
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_payload: Value = serde_json::from_slice(
        &to_bytes(put_resp.into_body(), usize::MAX)
            .await
            .expect("put body"),
    )
    .expect("put json");
    let source_memory_id = put_payload
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .expect("source memory id");

    let mut candidate = sample_candidate(
        "wflearn-promote",
        "workflow-promote",
        crate::WorkflowLearningCandidateKind::MemoryFact,
        crate::WorkflowLearningCandidateStatus::Approved,
    );
    candidate.source_run_id = "wflearn-promote-run".to_string();
    candidate.source_memory_id = Some(source_memory_id.clone());
    state
        .put_workflow_learning_candidate(candidate)
        .await
        .expect("put promote candidate");

    let promote_req = Request::builder()
        .method("POST")
        .uri("/workflow-learning/candidates/wflearn-promote/promote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "run_id": "wflearn-promote-run",
                "reason": "promote approved learning"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");
    assert_eq!(
        promote_payload
            .get("candidate")
            .and_then(|row| row.get("source_memory_id"))
            .and_then(Value::as_str),
        Some(source_memory_id.as_str())
    );
    assert!(promote_payload
        .get("candidate")
        .and_then(|row| row.get("promoted_memory_id"))
        .and_then(Value::as_str)
        .is_some());
    assert_eq!(
        promote_payload
            .get("promotion")
            .and_then(|row| row.get("promoted"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let missing_req = Request::builder()
        .method("POST")
        .uri("/workflow-learning/candidates/missing-candidate/promote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1"
            })
            .to_string(),
        ))
        .expect("missing promote request");
    let missing_resp = app
        .oneshot(missing_req)
        .await
        .expect("missing promote response");
    assert_eq!(missing_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workflow_learning_candidate_spawn_revision_marks_missing_plan_bundle() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!("wflearn-workspace-{}", uuid::Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    state
        .put_automation_v2(sample_automation(&workspace_root, "workflow-revision"))
        .await
        .expect("put automation");
    state
        .put_workflow_learning_candidate(sample_candidate(
            "wflearn-revision",
            "workflow-revision",
            crate::WorkflowLearningCandidateKind::PromptPatch,
            crate::WorkflowLearningCandidateStatus::Approved,
        ))
        .await
        .expect("put revision candidate");

    let spawn_req = Request::builder()
        .method("POST")
        .uri("/workflow-learning/candidates/wflearn-revision/spawn-revision")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reviewer_id": "reviewer-1",
                "title": "Revise workflow"
            })
            .to_string(),
        ))
        .expect("spawn request");
    let spawn_resp = app
        .clone()
        .oneshot(spawn_req)
        .await
        .expect("spawn response");
    assert_eq!(spawn_resp.status(), StatusCode::CONFLICT);
    let updated = state
        .get_workflow_learning_candidate("wflearn-revision")
        .await
        .expect("updated candidate");
    assert!(updated.needs_plan_bundle);

    let missing_req = Request::builder()
        .method("POST")
        .uri("/workflow-learning/candidates/missing-candidate/spawn-revision")
        .header("content-type", "application/json")
        .body(Body::from(json!({"reviewer_id": "reviewer-1"}).to_string()))
        .expect("missing spawn request");
    let missing_resp = app
        .oneshot(missing_req)
        .await
        .expect("missing spawn response");
    assert_eq!(missing_resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workflow_learning_candidate_spawn_revision_creates_planner_session_with_context() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!(
            "wflearn-workspace-session-{}",
            uuid::Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string();
    let mut automation = sample_automation(&workspace_root, "workflow-revision-session");
    automation.metadata = Some(json!({
        "plan_package_bundle": sample_plan_package_bundle()
    }));
    state
        .put_automation_v2(automation)
        .await
        .expect("put automation");
    let mut candidate = sample_candidate(
        "wflearn-revision-session",
        "workflow-revision-session",
        crate::WorkflowLearningCandidateKind::PromptPatch,
        crate::WorkflowLearningCandidateStatus::Approved,
    );
    candidate.summary = "Tighten the node prompt around required citations".to_string();
    candidate.fingerprint = "fingerprint-revision-session".to_string();
    candidate.run_ids = vec!["run-a".to_string(), "run-b".to_string()];
    candidate.evidence_refs = vec![json!({
        "run_id": "run-b",
        "node_id": "node-1",
        "reason": "validator rejected unsupported citations"
    })];
    state
        .put_workflow_learning_candidate(candidate)
        .await
        .expect("put revision candidate");

    let spawn_req = Request::builder()
        .method("POST")
        .uri("/workflow-learning/candidates/wflearn-revision-session/spawn-revision")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reviewer_id": "reviewer-1",
                "title": "Revise workflow from learning"
            })
            .to_string(),
        ))
        .expect("spawn request");
    let spawn_resp = app
        .clone()
        .oneshot(spawn_req)
        .await
        .expect("spawn response");
    assert_eq!(spawn_resp.status(), StatusCode::OK);
    let spawn_payload: Value = serde_json::from_slice(
        &to_bytes(spawn_resp.into_body(), usize::MAX)
            .await
            .expect("spawn body"),
    )
    .expect("spawn json");
    let session_id = spawn_payload
        .get("session")
        .and_then(|row| row.get("session_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .expect("session id");
    let notes = spawn_payload
        .get("session")
        .and_then(|row| row.get("notes"))
        .and_then(Value::as_str)
        .expect("session notes");
    assert!(notes.contains("fingerprint-revision-session"));
    assert!(notes.contains("run-a, run-b"));
    assert!(notes.contains("Preserve validated parts of the existing workflow"));
    assert!(notes.contains("do not regress completion rate or validation pass rate"));
    assert_eq!(
        spawn_payload
            .get("session")
            .and_then(|row| row.get("title"))
            .and_then(Value::as_str),
        Some("Revise workflow from learning")
    );
    assert_eq!(
        spawn_payload
            .get("session")
            .and_then(|row| row.get("source_kind"))
            .and_then(Value::as_str),
        Some("workflow_learning_revision")
    );
    assert_eq!(
        spawn_payload
            .get("session")
            .and_then(|row| row.get("import_validation"))
            .and_then(|row| row.get("compatible"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(spawn_payload
        .get("session")
        .and_then(|row| row.get("draft"))
        .is_some());

    let stored_session = state
        .get_workflow_planner_session(&session_id)
        .await
        .expect("stored planner session");
    assert_eq!(stored_session.session_id, session_id);
    assert!(stored_session
        .notes
        .contains("validator rejected unsupported citations"));
    let updated_candidate = state
        .get_workflow_learning_candidate("wflearn-revision-session")
        .await
        .expect("updated candidate");
    assert_eq!(
        updated_candidate.last_revision_session_id.as_deref(),
        Some(session_id.as_str())
    );
    assert!(updated_candidate.baseline_before.is_some());
}

#[tokio::test]
async fn workflow_learning_graph_patch_spawn_revision_tracks_change_type_metadata() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!(
            "wflearn-workspace-graph-session-{}",
            uuid::Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string();
    let mut automation = sample_automation(&workspace_root, "workflow-graph-session");
    automation.metadata = Some(json!({
        "plan_package_bundle": sample_plan_package_bundle()
    }));
    state
        .put_automation_v2(automation)
        .await
        .expect("put automation");
    let mut candidate = sample_candidate(
        "wflearn-graph-session",
        "workflow-graph-session",
        crate::WorkflowLearningCandidateKind::GraphPatch,
        crate::WorkflowLearningCandidateStatus::Approved,
    );
    candidate.summary = "Split the synthesis node and adjust dependency boundaries".to_string();
    candidate.fingerprint = "fingerprint-graph-session".to_string();
    candidate.run_ids = vec!["run-graph-a".to_string(), "run-graph-b".to_string()];
    candidate.evidence_refs = vec![json!({
        "run_id": "run-graph-b",
        "node_id": "node-1",
        "reason": "validator failed after repeated synthesis retries"
    })];
    state
        .put_workflow_learning_candidate(candidate)
        .await
        .expect("put graph candidate");

    let spawn_req = Request::builder()
        .method("POST")
        .uri("/workflow-learning/candidates/wflearn-graph-session/spawn-revision")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reviewer_id": "reviewer-graph",
                "title": "Restructure workflow from graph learning"
            })
            .to_string(),
        ))
        .expect("spawn request");
    let spawn_resp = app
        .clone()
        .oneshot(spawn_req)
        .await
        .expect("spawn response");
    assert_eq!(spawn_resp.status(), StatusCode::OK);
    let spawn_payload: Value = serde_json::from_slice(
        &to_bytes(spawn_resp.into_body(), usize::MAX)
            .await
            .expect("spawn body"),
    )
    .expect("spawn json");
    assert_eq!(
        spawn_payload
            .get("session")
            .and_then(|row| row.get("operator_preferences"))
            .and_then(|row| row.get("requested_change_type"))
            .and_then(Value::as_str),
        Some("graph_patch")
    );
    let notes = spawn_payload
        .get("session")
        .and_then(|row| row.get("notes"))
        .and_then(Value::as_str)
        .expect("graph notes");
    assert!(notes.contains("Requested change type: graph_patch."));
    assert!(notes.contains("run-graph-a, run-graph-b"));
    assert!(notes.contains("fingerprint-graph-session"));
    let session_id = spawn_payload
        .get("session")
        .and_then(|row| row.get("session_id"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .expect("session id");
    let stored_session = state
        .get_workflow_planner_session(&session_id)
        .await
        .expect("stored planner session");
    assert_eq!(
        stored_session
            .operator_preferences
            .as_ref()
            .and_then(|row| row.get("requested_change_type"))
            .and_then(Value::as_str),
        Some("graph_patch")
    );
    let updated_candidate = state
        .get_workflow_learning_candidate("wflearn-graph-session")
        .await
        .expect("updated graph candidate");
    assert_eq!(updated_candidate.needs_plan_bundle, false);
    assert_eq!(
        updated_candidate.last_revision_session_id.as_deref(),
        Some(session_id.as_str())
    );
}

#[tokio::test]
async fn context_distill_persists_and_dedupes_session_memory_facts() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind listener");
    let addr = listener.local_addr().expect("local addr");
    let provider_app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(|| async {
            axum::Json(json!({
                "choices": [
                    {
                        "message": {
                            "content": r#"[{"category":"fact","content":"The user prefers concise release summaries with explicit validation notes.","importance":0.91,"follow_up_needed":false}]"#
                        }
                    }
                ]
            }))
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, provider_app)
            .await
            .expect("serve test provider");
    });

    let state = test_state().await;
    state
        .config
        .patch_project(json!({
            "default_provider": "openai",
            "providers": {
                "openai": {
                    "url": format!("http://{addr}/v1")
                }
            }
        }))
        .await
        .expect("patch project");
    state
        .auth
        .write()
        .await
        .insert("openai".to_string(), "test-key".to_string());
    state
        .providers
        .reload(state.config.get().await.into())
        .await;

    let app = app_router(state.clone());
    let request_body = json!({
        "session_id": "distill-session-1",
        "conversation": [
            "We are reviewing the release train, the workflow learning rollout, and the API notes in enough detail to preserve durable context for future runs.",
            "Please remember that the user prefers concise release summaries with explicit validation notes, risk callouts, and direct references to workflow-learning status."
        ],
        "run_id": "distill-run-1",
        "workflow_id": "workflow-distill-1",
        "project_id": "proj-distill-1",
        "artifact_refs": ["artifact://distill/report.md"],
        "subject": "default",
        "importance_threshold": 0.5
    });

    let first_req = Request::builder()
        .method("POST")
        .uri("/memory/context/distill")
        .header("content-type", "application/json")
        .body(Body::from(request_body.to_string()))
        .expect("first request");
    let first_resp = app
        .clone()
        .oneshot(first_req)
        .await
        .expect("first response");
    assert_eq!(first_resp.status(), StatusCode::OK);
    let first_payload: Value = serde_json::from_slice(
        &to_bytes(first_resp.into_body(), usize::MAX)
            .await
            .expect("first body"),
    )
    .expect("first json");
    assert_eq!(
        first_payload.get("facts_extracted").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        first_payload.get("stored_count").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        first_payload.get("deduped_count").and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        first_payload.get("status").and_then(Value::as_str),
        Some("stored")
    );
    let first_memory_ids = first_payload
        .get("memory_ids")
        .and_then(Value::as_array)
        .cloned()
        .expect("first memory ids");
    let first_candidate_ids = first_payload
        .get("candidate_ids")
        .and_then(Value::as_array)
        .cloned()
        .expect("first candidate ids");
    assert_eq!(first_memory_ids.len(), 1);
    assert_eq!(first_candidate_ids.len(), 1);

    let list_req = Request::builder()
        .method("GET")
        .uri("/memory?project_id=proj-distill-1&q=concise%20release%20summaries")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_payload: Value = serde_json::from_slice(
        &to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list body"),
    )
    .expect("list json");
    let items = list_payload
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .expect("memory items");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get("kind").and_then(Value::as_str), Some("fact"));
    assert_eq!(
        items[0].get("tier").and_then(Value::as_str),
        Some("session")
    );
    assert_eq!(
        items[0]
            .get("metadata")
            .and_then(|row| row.get("origin"))
            .and_then(Value::as_str),
        Some("session_distillation")
    );
    assert_eq!(
        items[0]
            .get("metadata")
            .and_then(|row| row.get("workflow_id"))
            .and_then(Value::as_str),
        Some("workflow-distill-1")
    );

    let second_req = Request::builder()
        .method("POST")
        .uri("/memory/context/distill")
        .header("content-type", "application/json")
        .body(Body::from(request_body.to_string()))
        .expect("second request");
    let second_resp = app
        .clone()
        .oneshot(second_req)
        .await
        .expect("second response");
    server.abort();

    assert_eq!(second_resp.status(), StatusCode::OK);
    let second_payload: Value = serde_json::from_slice(
        &to_bytes(second_resp.into_body(), usize::MAX)
            .await
            .expect("second body"),
    )
    .expect("second json");
    assert_eq!(
        second_payload.get("stored_count").and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        second_payload.get("deduped_count").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        second_payload.get("status").and_then(Value::as_str),
        Some("stored")
    );
    assert_eq!(
        second_payload.get("memory_ids"),
        Some(&Value::Array(first_memory_ids.clone()))
    );
    assert_eq!(
        second_payload.get("candidate_ids"),
        Some(&Value::Array(first_candidate_ids.clone()))
    );

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/workflow-learning/candidates?workflow_id=workflow-distill-1&kind=memory_fact")
        .body(Body::empty())
        .expect("candidates request");
    let candidates_resp = app
        .oneshot(candidates_req)
        .await
        .expect("candidates response");
    assert_eq!(candidates_resp.status(), StatusCode::OK);
    let candidates_payload: Value = serde_json::from_slice(
        &to_bytes(candidates_resp.into_body(), usize::MAX)
            .await
            .expect("candidates body"),
    )
    .expect("candidates json");
    assert_eq!(
        candidates_payload.get("count").and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        candidates_payload
            .get("candidates")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("candidate_id")),
        Some(&first_candidate_ids[0])
    );
}
