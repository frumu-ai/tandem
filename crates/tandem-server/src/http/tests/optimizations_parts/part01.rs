fn current_test_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_millis() as u64
}

fn sample_automation(workspace_root: &str) -> crate::AutomationV2Spec {
    crate::AutomationV2Spec {
        automation_id: "wf-opt".to_string(),
        name: "Optimization Target".to_string(),
        description: Some("A saved workflow for optimizer tests".to_string()),
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
                objective: "Write a clear report for the user".to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(crate::AutomationFlowOutputContract {
                    kind: "report".to_string(),
                    validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
                    enforcement: None,
                    schema: None,
                    summary_guidance: Some("Summarize the report clearly.".to_string()),
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

fn sample_automation_without_validator(workspace_root: &str) -> crate::AutomationV2Spec {
    let mut workflow = sample_automation(workspace_root);
    workflow.flow.nodes[0].output_contract = None;
    workflow
}

fn valid_objective_md() -> &'static str {
    "# Objective\n\nImprove validator-backed workflow quality without mutating live workflow state.\n"
}

fn valid_eval_yaml() -> &'static str {
    "pack_ref: eval-pack.jsonl
primary_metric: artifact_validator_pass_rate
secondary_metric: unmet_requirement_count
hard_guardrails:
  - blocked_node_rate
  - budget_ceilings
campaign_start_baseline_runs: 2
baseline_replay_every_candidates: 5
baseline_replay_every_minutes: 30
"
}

fn valid_mutation_policy_yaml() -> &'static str {
    "max_nodes_changed_per_candidate: 1
max_field_families_changed_per_candidate: 1
allowed_text_fields:
  - objective
  - output_contract_summary_guidance
allowed_knob_fields:
  - timeout_ms
  - retry_policy_max_attempts
  - retry_policy_retries
max_text_delta_chars: 300
max_text_delta_ratio: 0.25
timeout_delta_percent: 0.15
timeout_delta_ms: 30000
timeout_min_ms: 30000
timeout_max_ms: 600000
retry_delta: 1
retry_min: 0
retry_max: 3
allow_text_and_knob_bundle: false
"
}

fn valid_scope_yaml() -> &'static str {
    "candidate_snapshot_only: true
allow_live_source_mutation: false
allow_external_side_effects_in_eval: false
promotion_requires_operator_approval: true
forbidden_fields:
  - flow.nodes[].depends_on
  - flow.nodes[].gate
  - agents
  - output_targets
"
}

fn valid_budget_yaml() -> &'static str {
    "max_experiments: 20
max_runtime_minutes: 120
max_consecutive_failures: 3
max_total_tokens: 500000
max_total_cost_usd: 25.0
"
}

fn write_phase1_artifacts(workspace_root: &std::path::Path) {
    std::fs::write(workspace_root.join("objective.md"), valid_objective_md()).expect("objective");
    std::fs::write(workspace_root.join("eval.yaml"), valid_eval_yaml()).expect("eval");
    std::fs::write(
        workspace_root.join("mutation_policy.yaml"),
        valid_mutation_policy_yaml(),
    )
    .expect("mutation");
    std::fs::write(workspace_root.join("scope.yaml"), valid_scope_yaml()).expect("scope");
    std::fs::write(workspace_root.join("budget.yaml"), valid_budget_yaml()).expect("budget");
}

#[tokio::test]
async fn optimizations_create_clones_saved_workflow_snapshot() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    state
        .put_automation_v2(sample_automation(
            workspace_root.to_str().expect("workspace root"),
        ))
        .await
        .expect("seed automation");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/optimizations")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "source_workflow_id": "wf-opt",
                "artifacts": {
                    "objective_ref": "objective.md",
                    "eval_ref": "eval.yaml",
                    "mutation_policy_ref": "mutation_policy.yaml",
                    "scope_ref": "scope.yaml",
                    "budget_ref": "budget.yaml"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("optimization")
            .and_then(|row| row.get("source_workflow_id"))
            .and_then(Value::as_str),
        Some("wf-opt")
    );
    assert_eq!(
        payload
            .get("optimization")
            .and_then(|row| row.get("baseline_snapshot"))
            .and_then(|row| row.get("automation_id"))
            .and_then(Value::as_str),
        Some("wf-opt")
    );
    assert_eq!(
        payload
            .get("optimization")
            .and_then(|row| row.get("frozen_artifacts"))
            .and_then(|row| row.get("objective"))
            .and_then(|row| row.get("artifact_ref"))
            .and_then(Value::as_str),
        Some("objective.md")
    );
    assert_eq!(
        payload
            .get("optimization")
            .and_then(|row| row.get("phase1"))
            .and_then(|row| row.get("eval"))
            .and_then(|row| row.get("primary_metric"))
            .and_then(Value::as_str),
        Some("artifact_validator_pass_rate")
    );
    assert_eq!(
        payload.get("experimentCount").and_then(Value::as_u64),
        Some(0)
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_create_applies_execution_override_only_to_campaign_baseline() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    state
        .put_automation_v2(sample_automation(
            workspace_root.to_str().expect("workspace root"),
        ))
        .await
        .expect("seed automation");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/optimizations")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "source_workflow_id": "wf-opt",
                "execution_override": {
                    "provider_id": "local",
                    "model_id": "echo-1"
                },
                "artifacts": {
                    "objective_ref": "objective.md",
                    "eval_ref": "eval.yaml",
                    "mutation_policy_ref": "mutation_policy.yaml",
                    "scope_ref": "scope.yaml",
                    "budget_ref": "budget.yaml"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("optimization")
            .and_then(|row| row.get("execution_override"))
            .and_then(|row| row.get("provider_id"))
            .and_then(Value::as_str),
        Some("local")
    );
    assert_eq!(
        payload
            .get("optimization")
            .and_then(|row| row.get("baseline_snapshot"))
            .and_then(|row| row.get("agents"))
            .and_then(Value::as_array)
            .and_then(|agents| agents.first())
            .and_then(|agent| agent.get("model_policy"))
            .and_then(|policy| policy.get("default_model"))
            .and_then(|model| model.get("model_id"))
            .and_then(Value::as_str),
        Some("echo-1")
    );
    assert_eq!(
        payload
            .get("optimization")
            .and_then(|row| row.get("source_workflow_snapshot"))
            .and_then(|row| row.get("agents"))
            .and_then(Value::as_array)
            .and_then(|agents| agents.first())
            .and_then(|agent| agent.get("model_policy")),
        None
    );
    let source = state
        .get_automation_v2("wf-opt")
        .await
        .expect("source workflow");
    assert!(source.agents[0].model_policy.is_none());
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_list_returns_campaigns_sorted_by_update_time() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    let phase1 = crate::load_optimization_phase1_config(&frozen_artifacts).expect("phase1");
    state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-older".to_string(),
            name: "Older".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::Draft,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(phase1.clone()),
            baseline_metrics: None,
            baseline_replays: Vec::new(),
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: None,
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("older campaign");
    state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-newer".to_string(),
            name: "Newer".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::Running,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts,
            phase1: Some(phase1),
            baseline_metrics: None,
            baseline_replays: Vec::new(),
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: None,
            last_pause_reason: None,
            created_at_ms: 2,
            updated_at_ms: 2,
            metadata: None,
        })
        .await
        .expect("newer campaign");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/optimizations")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let rows = payload
        .get("optimizations")
        .and_then(Value::as_array)
        .expect("optimizations array");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(2));
    assert_eq!(
        rows.first()
            .and_then(|row| row.get("optimization_id"))
            .and_then(Value::as_str),
        Some("opt-newer")
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_experiments_list_returns_campaign_experiments() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    let phase1 = crate::load_optimization_phase1_config(&frozen_artifacts).expect("phase1");
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    let campaign = state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-exp".to_string(),
            name: "Experiments".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::Running,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts,
            phase1: Some(phase1),
            baseline_metrics: None,
            baseline_replays: Vec::new(),
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: None,
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("campaign");
    let mut candidate = campaign.baseline_snapshot.clone();
    candidate.flow.nodes[0].objective = "Write a sharper report".to_string();
    state
        .put_optimization_experiment(crate::OptimizationExperimentRecord {
            experiment_id: "exp-list-1".to_string(),
            optimization_id: campaign.optimization_id.clone(),
            status: crate::OptimizationExperimentStatus::Completed,
            candidate_snapshot: candidate,
            candidate_snapshot_hash: "".to_string(),
            baseline_snapshot_hash: campaign.baseline_snapshot_hash.clone(),
            mutation_summary: Some("objective delta".to_string()),
            metrics: None,
            phase1_metrics: None,
            promotion_recommendation: None,
            promotion_decision: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("experiment");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("GET")
        .uri("/optimizations/opt-exp/experiments")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("count").and_then(Value::as_u64), Some(1));
    assert_eq!(
        payload
            .get("experiments")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("experiment_id"))
            .and_then(Value::as_str),
        Some("exp-list-1")
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_approve_winner_updates_campaign_baseline_without_mutating_source_workflow() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    let campaign = state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-1".to_string(),
            name: "Optimize Workflow".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::AwaitingPromotionApproval,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: None,
            baseline_replays: vec![crate::OptimizationBaselineReplayRecord {
                replay_id: "replay-1".to_string(),
                automation_run_id: Some("run-1".to_string()),
                phase1_metrics: crate::OptimizationPhase1Metrics {
                    artifact_validator_pass_rate: 1.0,
                    unmet_requirement_count: 0.0,
                    blocked_node_rate: 0.0,
                    budget_within_limits: true,
                },
                validator_case_outcomes: std::collections::BTreeMap::from([(
                    "node-1".to_string(),
                    "passed".to_string(),
                )]),
                experiment_count_at_recording: 0,
                recorded_at_ms: 1,
            }],
            pending_baseline_run_ids: vec!["run-pending".to_string()],
            pending_promotion_experiment_id: Some("exp-1".to_string()),
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed campaign");
    let mut candidate = campaign.baseline_snapshot.clone();
    candidate.flow.nodes[0].objective = "Write a clear report for the team".to_string();
    state
        .put_optimization_experiment(crate::OptimizationExperimentRecord {
            experiment_id: "exp-1".to_string(),
            optimization_id: campaign.optimization_id.clone(),
            status: crate::OptimizationExperimentStatus::PromotionRecommended,
            candidate_snapshot: candidate.clone(),
            candidate_snapshot_hash: crate::optimization_snapshot_hash(&candidate),
            baseline_snapshot_hash: campaign.baseline_snapshot_hash.clone(),
            mutation_summary: Some("tighten node objective".to_string()),
            metrics: Some(json!({"validator_pass_rate": 1.0})),
            phase1_metrics: None,
            promotion_recommendation: Some("promote".to_string()),
            promotion_decision: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed experiment");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-1/actions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "approve_winner",
                "experiment_id": "exp-1"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let campaign = state
        .get_optimization_campaign("opt-1")
        .await
        .expect("campaign");
    assert_eq!(
        campaign.baseline_snapshot.flow.nodes[0].objective,
        "Write a clear report for the team"
    );
    let source_after = state
        .get_automation_v2("wf-opt")
        .await
        .expect("source workflow");
    assert_eq!(
        source_after.flow.nodes[0].objective,
        "Write a clear report for the user"
    );
    assert!(campaign.baseline_replays.is_empty());
    assert!(campaign.pending_baseline_run_ids.is_empty());
    let experiment = state
        .get_optimization_experiment("opt-1", "exp-1")
        .await
        .expect("experiment");
    assert_eq!(
        experiment
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("apply_patch"))
            .and_then(|patch| patch.get("field_path"))
            .and_then(Value::as_str),
        Some("objective")
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_apply_winner_updates_live_workflow_and_records_audit_metadata() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    let campaign = state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-apply".to_string(),
            name: "Apply Winner".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::AwaitingPromotionApproval,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: None,
            baseline_replays: Vec::new(),
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: Some("exp-apply".to_string()),
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed campaign");
    let mut candidate = campaign.baseline_snapshot.clone();
    candidate.flow.nodes[0].objective = "Write a clear report for the team".to_string();
    state
        .put_optimization_experiment(crate::OptimizationExperimentRecord {
            experiment_id: "exp-apply".to_string(),
            optimization_id: campaign.optimization_id.clone(),
            status: crate::OptimizationExperimentStatus::PromotionRecommended,
            candidate_snapshot: candidate.clone(),
            candidate_snapshot_hash: crate::optimization_snapshot_hash(&candidate),
            baseline_snapshot_hash: campaign.baseline_snapshot_hash.clone(),
            mutation_summary: Some("tighten node objective".to_string()),
            metrics: Some(json!({"validator_pass_rate": 1.0})),
            phase1_metrics: None,
            promotion_recommendation: Some("promote".to_string()),
            promotion_decision: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed experiment");
    let app = app_router(state.clone());
    let approve = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-apply/actions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "approve_winner",
                "experiment_id": "exp-apply"
            })
            .to_string(),
        ))
        .expect("request");
    let approve_resp = app.clone().oneshot(approve).await.expect("response");
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let apply = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-apply/experiments/exp-apply")
        .body(Body::empty())
        .expect("request");
    let apply_resp = app.oneshot(apply).await.expect("response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let live = state
        .get_automation_v2("wf-opt")
        .await
        .expect("live workflow");
    assert_eq!(
        live.flow.nodes[0].objective,
        "Write a clear report for the team"
    );
    assert_eq!(
        live.metadata
            .as_ref()
            .and_then(|metadata| metadata.get("last_optimization_apply"))
            .and_then(|row| row.get("experiment_id"))
            .and_then(Value::as_str),
        Some("exp-apply")
    );
    let experiment = state
        .get_optimization_experiment("opt-apply", "exp-apply")
        .await
        .expect("experiment");
    assert_eq!(
        experiment
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("applied_to_live"))
            .and_then(|row| row.get("automation_id"))
            .and_then(Value::as_str),
        Some("wf-opt")
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_apply_winner_rejects_live_workflow_drift() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    let campaign = state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-drift".to_string(),
            name: "Apply Drift".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::AwaitingPromotionApproval,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: None,
            baseline_replays: Vec::new(),
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: Some("exp-drift".to_string()),
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed campaign");
    let mut candidate = campaign.baseline_snapshot.clone();
    candidate.flow.nodes[0].objective = "Write a clear report for the team".to_string();
    state
        .put_optimization_experiment(crate::OptimizationExperimentRecord {
            experiment_id: "exp-drift".to_string(),
            optimization_id: campaign.optimization_id.clone(),
            status: crate::OptimizationExperimentStatus::PromotionRecommended,
            candidate_snapshot: candidate.clone(),
            candidate_snapshot_hash: crate::optimization_snapshot_hash(&candidate),
            baseline_snapshot_hash: campaign.baseline_snapshot_hash.clone(),
            mutation_summary: Some("tighten node objective".to_string()),
            metrics: Some(json!({"validator_pass_rate": 1.0})),
            phase1_metrics: None,
            promotion_recommendation: Some("promote".to_string()),
            promotion_decision: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed experiment");
    let app = app_router(state.clone());
    let approve = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-drift/actions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "approve_winner",
                "experiment_id": "exp-drift"
            })
            .to_string(),
        ))
        .expect("request");
    let approve_resp = app.clone().oneshot(approve).await.expect("response");
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let mut drifted = state
        .get_automation_v2("wf-opt")
        .await
        .expect("live workflow");
    drifted.flow.nodes[0].objective = "A person changed this prompt manually".to_string();
    state
        .put_automation_v2(drifted)
        .await
        .expect("store drifted workflow");
    let apply = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-drift/experiments/exp-drift")
        .body(Body::empty())
        .expect("request");
    let apply_resp = app.oneshot(apply).await.expect("response");
    assert_eq!(apply_resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| error.contains("live workflow drift")));
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_approve_winner_rejects_candidate_with_multiple_field_families() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    let campaign = state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-2".to_string(),
            name: "Optimize Workflow".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::AwaitingPromotionApproval,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: None,
            baseline_replays: Vec::new(),
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: Some("exp-2".to_string()),
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed campaign");
    let mut candidate = campaign.baseline_snapshot.clone();
    candidate.flow.nodes[0].objective = "Write a clear report for the team".to_string();
    candidate.flow.nodes[0].timeout_ms = Some(65_000);
    state
        .put_optimization_experiment(crate::OptimizationExperimentRecord {
            experiment_id: "exp-2".to_string(),
            optimization_id: campaign.optimization_id.clone(),
            status: crate::OptimizationExperimentStatus::PromotionRecommended,
            candidate_snapshot: candidate.clone(),
            candidate_snapshot_hash: crate::optimization_snapshot_hash(&candidate),
            baseline_snapshot_hash: campaign.baseline_snapshot_hash.clone(),
            mutation_summary: None,
            metrics: Some(json!({"validator_pass_rate": 1.0})),
            phase1_metrics: None,
            promotion_recommendation: Some("promote".to_string()),
            promotion_decision: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed experiment");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-2/actions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "approve_winner",
                "experiment_id": "exp-2"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| error.contains("one field family")));
    let campaign_after = state
        .get_optimization_campaign("opt-2")
        .await
        .expect("campaign");
    assert_eq!(
        campaign_after.baseline_snapshot.flow.nodes[0].objective,
        "Write a clear report for the user"
    );
    assert_eq!(
        campaign_after.baseline_snapshot.flow.nodes[0].timeout_ms,
        Some(60_000)
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_approve_winner_rejects_candidate_with_worse_phase1_metrics() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    let campaign = state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-3".to_string(),
            name: "Optimize Workflow".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::AwaitingPromotionApproval,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: Some(crate::OptimizationPhase1Metrics {
                artifact_validator_pass_rate: 0.8,
                unmet_requirement_count: 1.0,
                blocked_node_rate: 0.0,
                budget_within_limits: true,
            }),
            baseline_replays: Vec::new(),
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: Some("exp-3".to_string()),
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed campaign");
    let mut candidate = campaign.baseline_snapshot.clone();
    candidate.flow.nodes[0].objective = "Write a clear report for the team".to_string();
    state
        .put_optimization_experiment(crate::OptimizationExperimentRecord {
            experiment_id: "exp-3".to_string(),
            optimization_id: campaign.optimization_id.clone(),
            status: crate::OptimizationExperimentStatus::PromotionRecommended,
            candidate_snapshot: candidate.clone(),
            candidate_snapshot_hash: crate::optimization_snapshot_hash(&candidate),
            baseline_snapshot_hash: campaign.baseline_snapshot_hash.clone(),
            mutation_summary: None,
            metrics: Some(json!({
                "artifact_validator_pass_rate": 0.7,
                "unmet_requirement_count": 0.0,
                "blocked_node_rate": 0.0,
                "budget_within_limits": true
            })),
            phase1_metrics: None,
            promotion_recommendation: None,
            promotion_decision: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed experiment");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-3/actions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "approve_winner",
                "experiment_id": "exp-3"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| error.contains("did not beat")));
    let experiment = state
        .get_optimization_experiment("opt-3", "exp-3")
        .await
        .expect("experiment");
    assert_eq!(
        experiment.promotion_recommendation.as_deref(),
        Some("discard")
    );
    let campaign_after = state
        .get_optimization_campaign("opt-3")
        .await
        .expect("campaign");
    assert_eq!(
        campaign_after.baseline_snapshot.flow.nodes[0].objective,
        "Write a clear report for the user"
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_start_establishes_stable_phase1_baseline() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-start-stable".to_string(),
            name: "Optimize Workflow".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::Draft,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: None,
            baseline_replays: vec![
                crate::OptimizationBaselineReplayRecord {
                    replay_id: "replay-1".to_string(),
                    automation_run_id: Some("run-1".to_string()),
                    phase1_metrics: crate::OptimizationPhase1Metrics {
                        artifact_validator_pass_rate: 0.8,
                        unmet_requirement_count: 1.0,
                        blocked_node_rate: 0.0,
                        budget_within_limits: true,
                    },
                    validator_case_outcomes: std::collections::BTreeMap::from([(
                        "node-1".to_string(),
                        "passed".to_string(),
                    )]),
                    experiment_count_at_recording: 0,
                    recorded_at_ms: 1,
                },
                crate::OptimizationBaselineReplayRecord {
                    replay_id: "replay-2".to_string(),
                    automation_run_id: Some("run-2".to_string()),
                    phase1_metrics: crate::OptimizationPhase1Metrics {
                        artifact_validator_pass_rate: 0.84,
                        unmet_requirement_count: 2.0,
                        blocked_node_rate: 0.02,
                        budget_within_limits: true,
                    },
                    validator_case_outcomes: std::collections::BTreeMap::from([(
                        "node-1".to_string(),
                        "passed".to_string(),
                    )]),
                    experiment_count_at_recording: 0,
                    recorded_at_ms: 2,
                },
            ],
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: None,
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed campaign");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-start-stable/actions")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "action": "start" }).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let campaign = state
        .get_optimization_campaign("opt-start-stable")
        .await
        .expect("campaign");
    assert_eq!(campaign.status, crate::OptimizationCampaignStatus::Running);
    let baseline = campaign.baseline_metrics.expect("baseline metrics");
    assert!((baseline.artifact_validator_pass_rate - 0.82).abs() < 1e-9);
    assert!((baseline.unmet_requirement_count - 1.5).abs() < 1e-9);
    assert!((baseline.blocked_node_rate - 0.01).abs() < 1e-9);
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_start_pauses_when_baseline_replay_is_unstable() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-start-unstable".to_string(),
            name: "Optimize Workflow".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::Draft,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: None,
            baseline_replays: vec![
                crate::OptimizationBaselineReplayRecord {
                    replay_id: "replay-1".to_string(),
                    automation_run_id: Some("run-1".to_string()),
                    phase1_metrics: crate::OptimizationPhase1Metrics {
                        artifact_validator_pass_rate: 0.8,
                        unmet_requirement_count: 1.0,
                        blocked_node_rate: 0.0,
                        budget_within_limits: true,
                    },
                    validator_case_outcomes: std::collections::BTreeMap::from([(
                        "node-1".to_string(),
                        "passed".to_string(),
                    )]),
                    experiment_count_at_recording: 0,
                    recorded_at_ms: 1,
                },
                crate::OptimizationBaselineReplayRecord {
                    replay_id: "replay-2".to_string(),
                    automation_run_id: Some("run-2".to_string()),
                    phase1_metrics: crate::OptimizationPhase1Metrics {
                        artifact_validator_pass_rate: 0.9,
                        unmet_requirement_count: 1.0,
                        blocked_node_rate: 0.0,
                        budget_within_limits: true,
                    },
                    validator_case_outcomes: std::collections::BTreeMap::from([(
                        "node-1".to_string(),
                        "passed".to_string(),
                    )]),
                    experiment_count_at_recording: 0,
                    recorded_at_ms: 2,
                },
            ],
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: None,
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed campaign");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-start-unstable/actions")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "action": "start" }).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let campaign = state
        .get_optimization_campaign("opt-start-unstable")
        .await
        .expect("campaign");
    assert_eq!(
        campaign.status,
        crate::OptimizationCampaignStatus::PausedEvaluatorUnstable
    );
    assert!(campaign
        .last_pause_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("artifact_validator_pass_rate")));
    assert!(campaign.baseline_metrics.is_none());
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_start_queues_initial_baseline_replay_when_missing() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-start-queue".to_string(),
            name: "Optimize Workflow".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::Draft,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: None,
            baseline_replays: Vec::new(),
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: None,
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed campaign");
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-start-queue/actions")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "action": "start" }).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let campaign = state
        .get_optimization_campaign("opt-start-queue")
        .await
        .expect("campaign");
    assert_eq!(campaign.status, crate::OptimizationCampaignStatus::Draft);
    assert_eq!(campaign.pending_baseline_run_ids.len(), 1);
    assert!(campaign.baseline_metrics.is_none());
    assert!(campaign
        .last_pause_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("baseline replay completion")));
    let run = state
        .get_automation_v2_run(&campaign.pending_baseline_run_ids[0])
        .await
        .expect("run");
    assert_eq!(run.trigger_type, "optimization_baseline_replay");
    assert_eq!(
        run.automation_snapshot
            .as_ref()
            .map(crate::optimization_snapshot_hash)
            .as_deref(),
        Some(campaign.baseline_snapshot_hash.as_str())
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimization_reconciler_ingests_completed_replays_and_establishes_baseline() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    let source = sample_automation(workspace_root.to_str().expect("workspace root"));
    let frozen_artifacts = crate::OptimizationFrozenArtifacts {
        objective: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "objective.md",
        )
        .expect("freeze objective"),
        eval: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "eval.yaml",
        )
        .expect("freeze eval"),
        mutation_policy: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "mutation_policy.yaml",
        )
        .expect("freeze mutation"),
        scope: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "scope.yaml",
        )
        .expect("freeze scope"),
        budget: crate::freeze_optimization_artifact(
            workspace_root.to_str().expect("workspace root"),
            "budget.yaml",
        )
        .expect("freeze budget"),
    };
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-reconcile".to_string(),
            name: "Optimize Workflow".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::Draft,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs {
                objective_ref: "objective.md".to_string(),
                eval_ref: "eval.yaml".to_string(),
                mutation_policy_ref: "mutation_policy.yaml".to_string(),
                scope_ref: "scope.yaml".to_string(),
                budget_ref: "budget.yaml".to_string(),
                research_log_ref: None,
                summary_ref: None,
            },
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: None,
            baseline_replays: Vec::new(),
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: None,
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("seed campaign");
    state
        .apply_optimization_action("opt-reconcile", "start", None, None, None)
        .await
        .expect("start campaign");
    let campaign = state
        .get_optimization_campaign("opt-reconcile")
        .await
        .expect("campaign");
    assert_eq!(campaign.pending_baseline_run_ids.len(), 1);
    let first_run_id = campaign.pending_baseline_run_ids[0].clone();
    state
        .update_automation_v2_run(&first_run_id, |row| {
            row.status = crate::AutomationRunStatus::Completed;
            row.started_at_ms = Some(1_000);
            row.finished_at_ms = Some(2_000);
            row.total_tokens = 100;
            row.estimated_cost_usd = 0.25;
            row.checkpoint.completed_nodes = vec!["node-1".to_string()];
            row.checkpoint.pending_nodes.clear();
            row.checkpoint.node_outputs.insert(
                "node-1".to_string(),
                json!({
                    "validator_summary": {
                        "outcome": "passed",
                        "unmet_requirements": []
                    }
                }),
            );
        })
        .await
        .expect("update run");
    let changed = state
        .reconcile_optimization_campaigns()
        .await
        .expect("reconcile");
    assert!(changed >= 1);
    let campaign = state
        .get_optimization_campaign("opt-reconcile")
        .await
        .expect("campaign");
    assert_eq!(campaign.baseline_replays.len(), 1);
    assert_eq!(campaign.pending_baseline_run_ids.len(), 1);
    let second_run_id = campaign.pending_baseline_run_ids[0].clone();
    assert_ne!(second_run_id, first_run_id);
    state
        .update_automation_v2_run(&second_run_id, |row| {
            row.status = crate::AutomationRunStatus::Completed;
            row.started_at_ms = Some(3_000);
            row.finished_at_ms = Some(4_000);
            row.total_tokens = 120;
            row.estimated_cost_usd = 0.30;
            row.checkpoint.completed_nodes = vec!["node-1".to_string()];
            row.checkpoint.pending_nodes.clear();
            row.checkpoint.node_outputs.insert(
                "node-1".to_string(),
                json!({
                    "validator_summary": {
                        "outcome": "passed",
                        "unmet_requirements": []
                    }
                }),
            );
        })
        .await
        .expect("update run");
    state
        .reconcile_optimization_campaigns()
        .await
        .expect("reconcile");
    let campaign = state
        .get_optimization_campaign("opt-reconcile")
        .await
        .expect("campaign");
    assert_eq!(campaign.pending_baseline_run_ids.len(), 0);
    assert_eq!(campaign.baseline_replays.len(), 2);
    assert_eq!(campaign.status, crate::OptimizationCampaignStatus::Running);
    let baseline = campaign.baseline_metrics.expect("baseline metrics");
    assert!((baseline.artifact_validator_pass_rate - 1.0).abs() < 1e-9);
    assert!((baseline.unmet_requirement_count - 0.0).abs() < 1e-9);
    assert!((baseline.blocked_node_rate - 0.0).abs() < 1e-9);
    let _ = std::fs::remove_dir_all(workspace_root);
}
