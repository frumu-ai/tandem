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
    kind: WorkflowLearningCandidateKind,
    status: WorkflowLearningCandidateStatus,
    fingerprint: &str,
) -> WorkflowLearningCandidate {
    let now = current_test_ms();
    WorkflowLearningCandidate {
        candidate_id: candidate_id.to_string(),
        workflow_id: workflow_id.to_string(),
        project_id: "proj-1".to_string(),
        source_run_id: format!("run-{candidate_id}"),
        kind,
        status,
        confidence: 0.5,
        summary: format!("summary for {candidate_id}"),
        fingerprint: fingerprint.to_string(),
        node_id: Some("node-1".to_string()),
        node_kind: Some("report_markdown".to_string()),
        validator_family: Some("research_brief".to_string()),
        evidence_refs: vec![json!({ "candidate_id": candidate_id })],
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

#[tokio::test]
async fn workflow_learning_candidate_upsert_dedupes_by_workflow_kind_and_fingerprint() {
    let state = ready_test_state().await;
    let first = sample_candidate(
        "wflearn-state-a",
        "workflow-a",
        WorkflowLearningCandidateKind::PromptPatch,
        WorkflowLearningCandidateStatus::Proposed,
        "shared-fingerprint",
    );
    let mut second = sample_candidate(
        "wflearn-state-b",
        "workflow-a",
        WorkflowLearningCandidateKind::PromptPatch,
        WorkflowLearningCandidateStatus::Proposed,
        "shared-fingerprint",
    );
    second.confidence = 0.9;
    second
        .artifact_refs
        .push("artifact://wflearn-state-b/extra.md".to_string());
    second.run_ids.push("run-extra".to_string());
    second
        .evidence_refs
        .push(json!({ "candidate_id": "wflearn-state-b", "extra": true }));

    let stored_first = state
        .upsert_workflow_learning_candidate(first)
        .await
        .expect("store first candidate");
    let stored_second = state
        .upsert_workflow_learning_candidate(second)
        .await
        .expect("upsert second candidate");

    assert_eq!(stored_second.candidate_id, stored_first.candidate_id);
    assert_eq!(stored_second.workflow_id, "workflow-a");
    assert_eq!(
        stored_second.kind,
        WorkflowLearningCandidateKind::PromptPatch
    );
    assert_eq!(stored_second.fingerprint, "shared-fingerprint");
    assert_eq!(stored_second.confidence, 0.9);
    assert!(stored_second
        .artifact_refs
        .iter()
        .any(|value| value == "artifact://wflearn-state-a/report.md"));
    assert!(stored_second
        .artifact_refs
        .iter()
        .any(|value| value == "artifact://wflearn-state-b/extra.md"));
    assert!(stored_second
        .run_ids
        .iter()
        .any(|value| value == "run-extra"));
    assert_eq!(
        state
            .list_workflow_learning_candidates(Some("workflow-a"), None, None)
            .await
            .len(),
        1
    );
}

#[tokio::test]
async fn workflow_learning_candidate_status_updates_roundtrip() {
    let state = ready_test_state().await;
    let candidate = sample_candidate(
        "wflearn-status",
        "workflow-status",
        WorkflowLearningCandidateKind::MemoryFact,
        WorkflowLearningCandidateStatus::Proposed,
        "status-fingerprint",
    );
    state
        .put_workflow_learning_candidate(candidate)
        .await
        .expect("put status candidate");

    let statuses = [
        WorkflowLearningCandidateStatus::Approved,
        WorkflowLearningCandidateStatus::Applied,
        WorkflowLearningCandidateStatus::Rejected,
        WorkflowLearningCandidateStatus::Regressed,
    ];
    for status in statuses {
        let updated = state
            .update_workflow_learning_candidate("wflearn-status", |candidate| {
                candidate.status = status;
            })
            .await
            .expect("updated candidate");
        assert_eq!(updated.status, status);
    }

    let stored = state
        .get_workflow_learning_candidate("wflearn-status")
        .await
        .expect("stored status candidate");
    assert_eq!(stored.status, WorkflowLearningCandidateStatus::Regressed);
}

#[tokio::test]
async fn automation_run_learning_summary_persists_to_state_and_status_json() {
    let state = ready_test_state().await;
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-workflow-learning-run-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace root");

    let automation = AutomationSpecBuilder::new("workflow-learning-run")
        .workspace_root(workspace_root.to_string_lossy().to_string())
        .nodes(vec![AutomationNodeBuilder::new("node-1").build()])
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("put automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");

    let expected_summary = WorkflowLearningRunSummary {
        generated_candidate_ids: vec!["wflearn-generated".to_string()],
        injected_learning_ids: vec!["wflearn-injected".to_string()],
        approved_learning_ids_considered: vec!["wflearn-approved".to_string()],
        post_run_metrics: Some(WorkflowLearningMetricsSnapshot {
            sample_size: 3,
            completion_rate: 1.0,
            validation_pass_rate: 1.0,
            mean_attempts_per_node: 1.0,
            repairable_failure_rate: 0.0,
            median_wall_clock_ms: 1200,
            human_intervention_count: 0,
            computed_at_ms: current_test_ms(),
        }),
    };
    let updated = state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = AutomationRunStatus::Completed;
            row.learning_summary = Some(expected_summary.clone());
        })
        .await
        .expect("update run");

    let stored = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("stored run");
    assert_eq!(stored.status, AutomationRunStatus::Completed);
    let stored_summary = stored
        .learning_summary
        .as_ref()
        .expect("stored learning summary");
    assert_eq!(
        stored_summary.generated_candidate_ids,
        expected_summary.generated_candidate_ids
    );
    assert_eq!(
        stored_summary.injected_learning_ids,
        expected_summary.injected_learning_ids
    );
    assert_eq!(
        stored_summary.approved_learning_ids_considered,
        expected_summary.approved_learning_ids_considered
    );
    assert_eq!(
        stored_summary
            .post_run_metrics
            .as_ref()
            .map(|metrics| metrics.sample_size),
        Some(3)
    );
    let updated_summary = updated
        .learning_summary
        .as_ref()
        .expect("updated learning summary");
    assert_eq!(
        updated_summary.injected_learning_ids,
        expected_summary.injected_learning_ids
    );

    let status_path = workspace_root
        .join(".tandem")
        .join("runs")
        .join(&run.run_id)
        .join("status.json");
    let status_payload: Value =
        serde_json::from_str(&std::fs::read_to_string(&status_path).expect("read status json"))
            .expect("status json");
    assert_eq!(
        status_payload
            .get("learning_summary")
            .and_then(|row| row.get("generated_candidate_ids"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        status_payload
            .get("learning_summary")
            .and_then(|row| row.get("injected_learning_ids"))
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("wflearn-injected")
    );
}

#[tokio::test]
async fn workflow_learning_context_only_surfaces_approved_candidates() {
    let state = ready_test_state().await;
    let automation = AutomationSpecBuilder::new("workflow-context")
        .metadata(json!({
            "project_id": "proj-1"
        }))
        .build();
    let node = AutomationNodeBuilder::new("node-1")
        .output_contract(AutomationFlowOutputContract {
            kind: "report".to_string(),
            validator: Some(AutomationOutputValidatorKind::ResearchBrief),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        })
        .build();

    let approved_same_workflow = sample_candidate(
        "wflearn-approved-same",
        "workflow-context",
        WorkflowLearningCandidateKind::MemoryFact,
        WorkflowLearningCandidateStatus::Approved,
        "ctx-approved-same",
    );
    let mut applied_project_candidate = sample_candidate(
        "wflearn-applied-project",
        "workflow-other",
        WorkflowLearningCandidateKind::PromptPatch,
        WorkflowLearningCandidateStatus::Applied,
        "ctx-applied-project",
    );
    applied_project_candidate.project_id = "proj-1".to_string();
    let proposed_same_workflow = sample_candidate(
        "wflearn-proposed",
        "workflow-context",
        WorkflowLearningCandidateKind::RepairHint,
        WorkflowLearningCandidateStatus::Proposed,
        "ctx-proposed",
    );

    state
        .put_workflow_learning_candidate(approved_same_workflow)
        .await
        .expect("put approved candidate");
    state
        .put_workflow_learning_candidate(applied_project_candidate)
        .await
        .expect("put applied project candidate");
    state
        .put_workflow_learning_candidate(proposed_same_workflow)
        .await
        .expect("put proposed candidate");

    let (candidate_ids, context) = state
        .workflow_learning_context_for_automation_node(&automation, &node)
        .await;

    assert_eq!(
        candidate_ids,
        vec![
            "wflearn-approved-same".to_string(),
            "wflearn-applied-project".to_string()
        ]
    );
    let context = context.expect("learning context");
    assert!(context.contains("<learning_context>"));
    assert!(context.contains("summary for wflearn-approved-same"));
    assert!(context.contains("summary for wflearn-applied-project"));
    assert!(!context.contains("summary for wflearn-proposed"));
}

#[tokio::test]
async fn record_automation_run_learning_usage_tracks_injected_ids() {
    let state = ready_test_state().await;
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-workflow-learning-usage-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace root");

    let automation = AutomationSpecBuilder::new("workflow-learning-usage")
        .workspace_root(workspace_root.to_string_lossy().to_string())
        .nodes(vec![AutomationNodeBuilder::new("node-1").build()])
        .build();
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");

    let updated = state
        .record_automation_v2_run_learning_usage(
            &run.run_id,
            &[
                "wflearn-approved-a".to_string(),
                "wflearn-approved-b".to_string(),
                "wflearn-approved-a".to_string(),
            ],
        )
        .await
        .expect("updated run");
    let summary = updated
        .learning_summary
        .as_ref()
        .expect("learning summary on updated run");
    assert_eq!(
        summary.injected_learning_ids,
        vec![
            "wflearn-approved-a".to_string(),
            "wflearn-approved-b".to_string()
        ]
    );
    assert_eq!(
        summary.approved_learning_ids_considered,
        vec![
            "wflearn-approved-a".to_string(),
            "wflearn-approved-b".to_string()
        ]
    );

    let status_path = workspace_root
        .join(".tandem")
        .join("runs")
        .join(&run.run_id)
        .join("status.json");
    let status_payload: Value =
        serde_json::from_str(&std::fs::read_to_string(&status_path).expect("read status json"))
            .expect("status json");
    assert_eq!(
        status_payload
            .get("learning_summary")
            .and_then(|row| row.get("approved_learning_ids_considered"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );
}

#[tokio::test]
async fn completed_runs_generate_memory_fact_candidates() {
    let state = ready_test_state().await;
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-workflow-learning-completed-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace root");

    let automation = AutomationSpecBuilder::new("workflow-learning-completed")
        .workspace_root(workspace_root.to_string_lossy().to_string())
        .nodes(vec![AutomationNodeBuilder::new("node-1").build()])
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("put automation");

    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let updated = state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = AutomationRunStatus::Completed;
            row.detail = Some("Remember that this workflow completed cleanly".to_string());
        })
        .await
        .expect("complete run");

    let candidates = state
        .list_workflow_learning_candidates(Some(&automation.automation_id), None, None)
        .await;
    assert_eq!(candidates.len(), 1);
    let candidate = &candidates[0];
    assert_eq!(candidate.kind, WorkflowLearningCandidateKind::MemoryFact);
    assert_eq!(candidate.status, WorkflowLearningCandidateStatus::Proposed);
    assert!(candidate.summary.contains("completed cleanly"));
    assert_eq!(
        updated
            .learning_summary
            .as_ref()
            .map(|summary| summary.generated_candidate_ids.len()),
        Some(1)
    );
}

#[tokio::test]
async fn repeated_failures_generate_deduped_repair_and_prompt_candidates_before_graph_patch() {
    let state = ready_test_state().await;
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-workflow-learning-failures-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace root");

    let automation = AutomationSpecBuilder::new("workflow-learning-failures")
        .workspace_root(workspace_root.to_string_lossy().to_string())
        .nodes(vec![AutomationNodeBuilder::new("node-1")
            .output_contract(AutomationFlowOutputContract {
                kind: "report".to_string(),
                validator: Some(AutomationOutputValidatorKind::ResearchBrief),
                enforcement: None,
                schema: None,
                summary_guidance: Some("Summarize the report.".to_string()),
            })
            .build()])
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("put automation");

    for index in 1..=2 {
        let run = state
            .create_automation_v2_run(&automation, "manual")
            .await
            .expect("create failed run");
        state
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::Failed;
                row.checkpoint.last_failure = Some(AutomationFailureRecord {
                    node_id: "node-1".to_string(),
                    reason: "validator rejected unsupported citations".to_string(),
                    failed_at_ms: current_test_ms() + index,
                });
            })
            .await
            .expect("mark failed run");
    }

    let after_two = state
        .list_workflow_learning_candidates(Some(&automation.automation_id), None, None)
        .await;
    assert_eq!(after_two.len(), 2);
    assert_eq!(
        after_two
            .iter()
            .filter(|candidate| candidate.kind == WorkflowLearningCandidateKind::RepairHint)
            .count(),
        1
    );
    assert_eq!(
        after_two
            .iter()
            .filter(|candidate| candidate.kind == WorkflowLearningCandidateKind::PromptPatch)
            .count(),
        1
    );
    assert_eq!(
        after_two
            .iter()
            .filter(|candidate| candidate.kind == WorkflowLearningCandidateKind::GraphPatch)
            .count(),
        0
    );

    let third_run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create third failed run");
    state
        .update_automation_v2_run(&third_run.run_id, |row| {
            row.status = AutomationRunStatus::Failed;
            row.checkpoint.last_failure = Some(AutomationFailureRecord {
                node_id: "node-1".to_string(),
                reason: "validator rejected unsupported citations".to_string(),
                failed_at_ms: current_test_ms() + 3,
            });
        })
        .await
        .expect("mark third failed run");

    let after_three = state
        .list_workflow_learning_candidates(Some(&automation.automation_id), None, None)
        .await;
    assert_eq!(
        after_three
            .iter()
            .filter(|candidate| candidate.kind == WorkflowLearningCandidateKind::RepairHint)
            .count(),
        1
    );
    assert_eq!(
        after_three
            .iter()
            .filter(|candidate| candidate.kind == WorkflowLearningCandidateKind::PromptPatch)
            .count(),
        1
    );
    assert_eq!(
        after_three
            .iter()
            .filter(|candidate| candidate.kind == WorkflowLearningCandidateKind::GraphPatch)
            .count(),
        1
    );
}
