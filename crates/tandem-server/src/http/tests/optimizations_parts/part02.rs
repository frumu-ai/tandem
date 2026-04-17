
#[tokio::test]
async fn optimization_reconciler_creates_candidate_eval_and_recommends_winner() {
    let state = test_state().await;
    let recent_replay_ms = current_test_ms();
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
            optimization_id: "opt-candidate".to_string(),
            name: "Optimize Workflow".to_string(),
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
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: Some(crate::OptimizationPhase1Metrics {
                artifact_validator_pass_rate: 0.5,
                unmet_requirement_count: 2.0,
                blocked_node_rate: 0.0,
                budget_within_limits: true,
            }),
            baseline_replays: vec![
                crate::OptimizationBaselineReplayRecord {
                    replay_id: "replay-1".to_string(),
                    automation_run_id: Some("run-1".to_string()),
                    phase1_metrics: crate::OptimizationPhase1Metrics {
                        artifact_validator_pass_rate: 0.5,
                        unmet_requirement_count: 2.0,
                        blocked_node_rate: 0.0,
                        budget_within_limits: true,
                    },
                    validator_case_outcomes: std::collections::BTreeMap::from([(
                        "node-1".to_string(),
                        "passed".to_string(),
                    )]),
                    experiment_count_at_recording: 0,
                    recorded_at_ms: recent_replay_ms.saturating_sub(1_000),
                },
                crate::OptimizationBaselineReplayRecord {
                    replay_id: "replay-2".to_string(),
                    automation_run_id: Some("run-2".to_string()),
                    phase1_metrics: crate::OptimizationPhase1Metrics {
                        artifact_validator_pass_rate: 0.5,
                        unmet_requirement_count: 2.0,
                        blocked_node_rate: 0.0,
                        budget_within_limits: true,
                    },
                    validator_case_outcomes: std::collections::BTreeMap::from([(
                        "node-1".to_string(),
                        "passed".to_string(),
                    )]),
                    experiment_count_at_recording: 0,
                    recorded_at_ms: recent_replay_ms,
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
    state
        .reconcile_optimization_campaigns()
        .await
        .expect("reconcile candidate creation");
    let experiments = state.list_optimization_experiments("opt-candidate").await;
    assert_eq!(experiments.len(), 1);
    let experiment = experiments.first().expect("experiment");
    assert_eq!(
        experiment.status,
        crate::OptimizationExperimentStatus::Draft
    );
    let eval_run_id = experiment
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("eval_run_id"))
        .and_then(Value::as_str)
        .expect("eval_run_id")
        .to_string();
    let eval_run = state
        .get_automation_v2_run(&eval_run_id)
        .await
        .expect("eval run");
    assert_eq!(eval_run.trigger_type, "optimization_candidate_eval");
    state
        .update_automation_v2_run(&eval_run_id, |row| {
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
        .expect("update eval run");
    state
        .reconcile_optimization_campaigns()
        .await
        .expect("reconcile eval completion");
    let experiment = state
        .get_optimization_experiment("opt-candidate", &experiment.experiment_id)
        .await
        .expect("experiment");
    assert_eq!(
        experiment.status,
        crate::OptimizationExperimentStatus::PromotionRecommended
    );
    assert_eq!(
        experiment.promotion_recommendation.as_deref(),
        Some("promote")
    );
    let campaign = state
        .get_optimization_campaign("opt-candidate")
        .await
        .expect("campaign");
    assert_eq!(
        campaign.status,
        crate::OptimizationCampaignStatus::AwaitingPromotionApproval
    );
    assert_eq!(
        campaign.pending_promotion_experiment_id.as_deref(),
        Some(experiment.experiment_id.as_str())
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimization_reconciler_rejects_candidate_eval_with_stale_baseline() {
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
            optimization_id: "opt-stale-baseline".to_string(),
            name: "Optimize Workflow".to_string(),
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
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(
                crate::load_optimization_phase1_config(&frozen_artifacts)
                    .expect("load phase1 config"),
            ),
            baseline_metrics: Some(crate::OptimizationPhase1Metrics {
                artifact_validator_pass_rate: 0.5,
                unmet_requirement_count: 2.0,
                blocked_node_rate: 0.0,
                budget_within_limits: true,
            }),
            baseline_replays: vec![
                crate::OptimizationBaselineReplayRecord {
                    replay_id: "replay-1".to_string(),
                    automation_run_id: Some("run-1".to_string()),
                    phase1_metrics: crate::OptimizationPhase1Metrics {
                        artifact_validator_pass_rate: 0.5,
                        unmet_requirement_count: 2.0,
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
                        artifact_validator_pass_rate: 0.5,
                        unmet_requirement_count: 2.0,
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
    let mut campaign = state
        .get_optimization_campaign("opt-stale-baseline")
        .await
        .expect("campaign");
    let mut mutated = campaign.baseline_snapshot.clone();
    mutated.flow.nodes[0].objective = "Write a sharper report".to_string();
    campaign.baseline_snapshot = mutated.clone();
    campaign.baseline_snapshot_hash = crate::optimization_snapshot_hash(&mutated);
    campaign.baseline_metrics = Some(crate::OptimizationPhase1Metrics {
        artifact_validator_pass_rate: 1.0,
        unmet_requirement_count: 0.0,
        blocked_node_rate: 0.0,
        budget_within_limits: true,
    });
    state
        .put_optimization_campaign(campaign)
        .await
        .expect("update campaign baseline");
    let candidate = mutated.clone();
    let eval_run = state
        .create_automation_v2_run(&candidate, "optimization_candidate_eval")
        .await
        .expect("create eval run");
    state
        .put_optimization_experiment(crate::OptimizationExperimentRecord {
            experiment_id: "exp-stale".to_string(),
            optimization_id: "opt-stale-baseline".to_string(),
            status: crate::OptimizationExperimentStatus::Draft,
            candidate_snapshot: candidate.clone(),
            candidate_snapshot_hash: crate::optimization_snapshot_hash(&candidate),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            mutation_summary: Some("objective delta".to_string()),
            metrics: None,
            phase1_metrics: None,
            promotion_recommendation: None,
            promotion_decision: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: Some(json!({
                "generator": "manual_test",
                "eval_run_id": eval_run.run_id,
            })),
        })
        .await
        .expect("seed experiment");
    state
        .update_automation_v2_run(&eval_run.run_id, |row| {
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
        .expect("update eval run");
    state
        .reconcile_optimization_campaigns()
        .await
        .expect("reconcile stale baseline");
    let updated = state
        .get_optimization_experiment("opt-stale-baseline", "exp-stale")
        .await
        .expect("experiment");
    assert_eq!(updated.status, crate::OptimizationExperimentStatus::Failed);
    assert!(updated
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("eval_failure"))
        .and_then(Value::as_object)
        .and_then(|row| row.get("reason"))
        .and_then(Value::as_str)
        .is_some_and(|reason| reason.contains("baseline_snapshot_hash")));
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimization_reconciler_queues_recurring_baseline_replay_after_candidate_interval() {
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
    state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-recurring-replay".to_string(),
            name: "Optimize Workflow".to_string(),
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
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(phase1),
            baseline_metrics: Some(crate::OptimizationPhase1Metrics {
                artifact_validator_pass_rate: 1.0,
                unmet_requirement_count: 0.0,
                blocked_node_rate: 0.0,
                budget_within_limits: true,
            }),
            baseline_replays: vec![
                crate::OptimizationBaselineReplayRecord {
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
                },
                crate::OptimizationBaselineReplayRecord {
                    replay_id: "replay-2".to_string(),
                    automation_run_id: Some("run-2".to_string()),
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
    for idx in 0..5 {
        let mut candidate = source.clone();
        candidate.flow.nodes[0].objective = format!("Variant {idx}");
        state
            .put_optimization_experiment(crate::OptimizationExperimentRecord {
                experiment_id: format!("exp-{idx}"),
                optimization_id: "opt-recurring-replay".to_string(),
                status: crate::OptimizationExperimentStatus::Discarded,
                candidate_snapshot: candidate.clone(),
                candidate_snapshot_hash: crate::optimization_snapshot_hash(&candidate),
                baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
                mutation_summary: Some("objective delta".to_string()),
                metrics: None,
                phase1_metrics: None,
                promotion_recommendation: Some("discard".to_string()),
                promotion_decision: None,
                created_at_ms: idx + 1,
                updated_at_ms: idx + 1,
                metadata: None,
            })
            .await
            .expect("seed experiment");
    }
    state
        .reconcile_optimization_campaigns()
        .await
        .expect("reconcile recurring replay");
    let campaign = state
        .get_optimization_campaign("opt-recurring-replay")
        .await
        .expect("campaign");
    assert_eq!(campaign.pending_baseline_run_ids.len(), 1);
    assert_eq!(campaign.status, crate::OptimizationCampaignStatus::Draft);
    assert_eq!(
        campaign.last_pause_reason.as_deref(),
        Some("waiting for phase 1 baseline replay completion")
    );
    let replay_run = state
        .get_automation_v2_run(&campaign.pending_baseline_run_ids[0])
        .await
        .expect("replay run");
    assert_eq!(replay_run.trigger_type, "optimization_baseline_replay");
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimization_reconciler_stops_after_max_consecutive_candidate_failures() {
    let state = test_state().await;
    let recent_replay_ms = current_test_ms();
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
    let mut phase1 = crate::load_optimization_phase1_config(&frozen_artifacts).expect("phase1");
    phase1.budget.max_consecutive_failures = 1;
    state
        .put_automation_v2(source.clone())
        .await
        .expect("seed automation");
    state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-failure-stop".to_string(),
            name: "Optimize Workflow".to_string(),
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
            frozen_artifacts: frozen_artifacts.clone(),
            phase1: Some(phase1),
            baseline_metrics: Some(crate::OptimizationPhase1Metrics {
                artifact_validator_pass_rate: 0.5,
                unmet_requirement_count: 2.0,
                blocked_node_rate: 0.0,
                budget_within_limits: true,
            }),
            baseline_replays: vec![
                crate::OptimizationBaselineReplayRecord {
                    replay_id: "replay-1".to_string(),
                    automation_run_id: Some("run-1".to_string()),
                    phase1_metrics: crate::OptimizationPhase1Metrics {
                        artifact_validator_pass_rate: 0.5,
                        unmet_requirement_count: 2.0,
                        blocked_node_rate: 0.0,
                        budget_within_limits: true,
                    },
                    validator_case_outcomes: std::collections::BTreeMap::from([(
                        "node-1".to_string(),
                        "passed".to_string(),
                    )]),
                    experiment_count_at_recording: 0,
                    recorded_at_ms: recent_replay_ms.saturating_sub(1_000),
                },
                crate::OptimizationBaselineReplayRecord {
                    replay_id: "replay-2".to_string(),
                    automation_run_id: Some("run-2".to_string()),
                    phase1_metrics: crate::OptimizationPhase1Metrics {
                        artifact_validator_pass_rate: 0.5,
                        unmet_requirement_count: 2.0,
                        blocked_node_rate: 0.0,
                        budget_within_limits: true,
                    },
                    validator_case_outcomes: std::collections::BTreeMap::from([(
                        "node-1".to_string(),
                        "passed".to_string(),
                    )]),
                    experiment_count_at_recording: 0,
                    recorded_at_ms: recent_replay_ms,
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
    state
        .reconcile_optimization_campaigns()
        .await
        .expect("reconcile candidate creation");
    let experiments = state
        .list_optimization_experiments("opt-failure-stop")
        .await;
    let experiment = experiments.first().expect("experiment");
    let eval_run_id = experiment
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("eval_run_id"))
        .and_then(Value::as_str)
        .expect("eval_run_id")
        .to_string();
    state
        .update_automation_v2_run(&eval_run_id, |row| {
            row.status = crate::AutomationRunStatus::Failed;
            row.started_at_ms = Some(1_000);
            row.finished_at_ms = Some(2_000);
        })
        .await
        .expect("update eval run");
    state
        .reconcile_optimization_campaigns()
        .await
        .expect("reconcile failure");
    let campaign = state
        .get_optimization_campaign("opt-failure-stop")
        .await
        .expect("campaign");
    assert_eq!(campaign.status, crate::OptimizationCampaignStatus::Failed);
    assert!(campaign
        .last_pause_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("consecutive failures")));
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_record_baseline_replay_from_run() {
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
            optimization_id: "opt-replay".to_string(),
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
    let queue_req = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-replay/actions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "queue_baseline_replay"
            })
            .to_string(),
        ))
        .expect("request");
    let queue_resp = app.clone().oneshot(queue_req).await.expect("response");
    assert_eq!(queue_resp.status(), StatusCode::OK);
    let queued_campaign = state
        .get_optimization_campaign("opt-replay")
        .await
        .expect("campaign");
    assert_eq!(queued_campaign.pending_baseline_run_ids.len(), 1);
    let run_id = queued_campaign.pending_baseline_run_ids[0].clone();
    let run = state.get_automation_v2_run(&run_id).await.expect("run");
    assert_eq!(run.trigger_type, "optimization_baseline_replay");
    assert_eq!(
        run.automation_snapshot
            .as_ref()
            .map(crate::optimization_snapshot_hash)
            .as_deref(),
        Some(queued_campaign.baseline_snapshot_hash.as_str())
    );
    state
        .update_automation_v2_run(&run.run_id, |row| {
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
    let req = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-replay/actions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "record_baseline_replay",
                "run_id": run.run_id
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let campaign = state
        .get_optimization_campaign("opt-replay")
        .await
        .expect("campaign");
    assert_eq!(campaign.baseline_replays.len(), 1);
    assert!(campaign.pending_baseline_run_ids.is_empty());
    let metrics = &campaign.baseline_replays[0].phase1_metrics;
    assert_eq!(
        campaign.baseline_replays[0].experiment_count_at_recording,
        0
    );
    assert_eq!(
        campaign.baseline_replays[0]
            .validator_case_outcomes
            .get("node-1")
            .map(String::as_str),
        Some("passed")
    );
    assert!((metrics.artifact_validator_pass_rate - 1.0).abs() < 1e-9);
    assert!((metrics.unmet_requirement_count - 0.0).abs() < 1e-9);
    assert!((metrics.blocked_node_rate - 0.0).abs() < 1e-9);
    assert!(metrics.budget_within_limits);
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_record_baseline_replay_rejects_mismatched_snapshot() {
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
            optimization_id: "opt-replay-mismatch".to_string(),
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
    let mut mismatched = source.clone();
    mismatched.flow.nodes[0].objective = "Write a clear report for the team".to_string();
    let run = state
        .create_automation_v2_run(&mismatched, "manual")
        .await
        .expect("create run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Completed;
            row.started_at_ms = Some(1_000);
            row.finished_at_ms = Some(2_000);
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
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri("/optimizations/opt-replay-mismatch/actions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "action": "record_baseline_replay",
                "run_id": run.run_id
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
        .is_some_and(|error| error.contains("baseline snapshot")));
    let campaign = state
        .get_optimization_campaign("opt-replay-mismatch")
        .await
        .expect("campaign");
    assert!(campaign.baseline_replays.is_empty());
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_create_rejects_artifacts_outside_workspace() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    let outside_root = std::env::temp_dir().join(format!("tandem-opt-outside-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    std::fs::create_dir_all(&outside_root).expect("create outside");
    std::fs::write(workspace_root.join("objective.md"), valid_objective_md()).expect("objective");
    std::fs::write(workspace_root.join("eval.yaml"), valid_eval_yaml()).expect("eval");
    std::fs::write(
        workspace_root.join("mutation_policy.yaml"),
        valid_mutation_policy_yaml(),
    )
    .expect("mutation");
    std::fs::write(workspace_root.join("scope.yaml"), valid_scope_yaml()).expect("scope");
    std::fs::write(outside_root.join("budget.yaml"), valid_budget_yaml()).expect("budget");
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
                    "budget_ref": outside_root.join("budget.yaml").to_string_lossy()
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let _ = std::fs::remove_dir_all(workspace_root);
    let _ = std::fs::remove_dir_all(outside_root);
}

#[tokio::test]
async fn optimizations_create_rejects_workflow_without_validator_backed_output() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    write_phase1_artifacts(&workspace_root);
    state
        .put_automation_v2(sample_automation_without_validator(
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
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| error.contains("validator-backed output contract")));
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn optimizations_create_rejects_mutation_policy_outside_phase1_caps() {
    let state = test_state().await;
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-opt-workspace-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    std::fs::write(workspace_root.join("objective.md"), valid_objective_md()).expect("objective");
    std::fs::write(workspace_root.join("eval.yaml"), valid_eval_yaml()).expect("eval");
    std::fs::write(
        workspace_root.join("mutation_policy.yaml"),
        "max_nodes_changed_per_candidate: 2
max_field_families_changed_per_candidate: 1
allowed_text_fields:
  - objective
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
",
    )
    .expect("mutation");
    std::fs::write(workspace_root.join("scope.yaml"), valid_scope_yaml()).expect("scope");
    std::fs::write(workspace_root.join("budget.yaml"), valid_budget_yaml()).expect("budget");
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
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload
        .get("error")
        .and_then(Value::as_str)
        .is_some_and(|error| error.contains("max_nodes_changed_per_candidate")));
    let _ = std::fs::remove_dir_all(workspace_root);
}
