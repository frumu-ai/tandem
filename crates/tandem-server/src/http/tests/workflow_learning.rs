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
