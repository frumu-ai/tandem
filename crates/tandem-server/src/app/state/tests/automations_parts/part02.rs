
#[tokio::test]
async fn automation_run_requeue_increments_attempt_counter() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-requeue-attempts-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("workspace");

    let automation = AutomationV2Spec {
        automation_id: "automation-inline-requeue-attempts".to_string(),
        name: "Requeue Attempt Increments".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![AutomationAgentProfile {
            agent_id: "agent_planner".to_string(),
            template_id: None,
            display_name: "Planner".to_string(),
            avatar_url: None,
            model_policy: Some(json!({
                "default_model": "openrouter/not-a-real-model"
            })),
            skills: Vec::new(),
            tool_policy: AutomationAgentToolPolicy {
                allowlist: vec!["*".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: AutomationFlowSpec {
            nodes: vec![AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "collect_inputs".to_string(),
                agent_id: "agent_planner".to_string(),
                objective: "Capture the report topic, delivery target, and formatting constraints."
                    .to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(AutomationFlowOutputContract {
                    kind: "brief".to_string(),
                    validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                    enforcement: None,
                    schema: None,
                    summary_guidance: None,
                }),
                retry_policy: None,
                timeout_ms: None,
                max_tool_calls: None,
                stage_kind: None,
                gate: None,
                metadata: Some(json!({
                    "inputs": {
                        "topic": "autonomous AI agentic workflows",
                        "delivery_email": "recipient@example.com",
                        "email_format": "simple html",
                        "attachments_allowed": false
                    }
                })),
            }],
        },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        creator_id: "test".to_string(),
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: Some(json!({
            "context_materialization": {
                "routines": [
                    {
                        "routine_id": "collect_inputs",
                        "visible_context_objects": [],
                        "step_context_bindings": [
                            {
                                "step_id": "collect_inputs",
                                "context_reads": ["ctx:collect_inputs:mission.goal"],
                                "context_writes": []
                            }
                        ]
                    }
                ]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };

    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();

    crate::automation_v2::executor::run_automation_v2_run(state.clone(), run).await;
    let first = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("first run");
    assert_eq!(
        first.checkpoint.node_attempts.get("collect_inputs"),
        Some(&1)
    );

    state
        .update_automation_v2_run(&run_id, |row| {
            row.status = AutomationRunStatus::Queued;
            row.detail = Some("requeue collect_inputs".to_string());
            row.resume_reason = Some("requeue collect_inputs".to_string());
            row.stop_kind = None;
            row.stop_reason = None;
            row.pause_reason = None;
            row.checkpoint.awaiting_gate = None;
            row.checkpoint.node_outputs.remove("collect_inputs");
            row.checkpoint
                .completed_nodes
                .retain(|node_id| node_id != "collect_inputs");
            row.checkpoint
                .blocked_nodes
                .retain(|node_id| node_id != "collect_inputs");
            if !row
                .checkpoint
                .pending_nodes
                .iter()
                .any(|node_id| node_id == "collect_inputs")
            {
                row.checkpoint
                    .pending_nodes
                    .push("collect_inputs".to_string());
            }
            if row
                .checkpoint
                .last_failure
                .as_ref()
                .is_some_and(|failure| failure.node_id == "collect_inputs")
            {
                row.checkpoint.last_failure = None;
            }
        })
        .await
        .expect("requeue run");

    let rerun = state.get_automation_v2_run(&run_id).await.expect("rerun");
    crate::automation_v2::executor::run_automation_v2_run(state.clone(), rerun).await;
    let second = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("second run");
    assert_eq!(
        second.checkpoint.node_attempts.get("collect_inputs"),
        Some(&2)
    );
    assert!(second
        .checkpoint
        .node_outputs
        .contains_key("collect_inputs"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn automation_run_requires_stored_runtime_context_partition_at_startup() {
    let automation = AutomationV2Spec {
        automation_id: "auto-runtime-context-test".to_string(),
        name: "Runtime Context Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(".".to_string()),
        metadata: Some(json!({
            "context_materialization": {
                "routines": [
                    {
                        "routine_id": "collect_inputs",
                        "visible_context_objects": [],
                        "step_context_bindings": []
                    }
                ]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let state = ready_test_state().await;
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("store automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.runtime_context = None;
        })
        .await
        .expect("clear runtime context");
    let stored_before_clear = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("stored run before clear");
    assert!(state
        .automation_v2_runtime_context(&stored_before_clear)
        .is_some());
    let stored_run = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("stored run");
    assert!(state.automation_v2_runtime_context(&stored_run).is_some());

    crate::automation_v2::executor::run_automation_v2_run(state.clone(), stored_run).await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Failed);
    assert_eq!(
        persisted.detail.as_deref(),
        Some("runtime context partition missing for automation run")
    );
}

#[tokio::test]
async fn automation_run_without_runtime_context_requirement_can_start_and_complete() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-automation-no-runtime-context-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("workspace");

    let automation = AutomationV2Spec {
        automation_id: "auto-no-runtime-context-test".to_string(),
        name: "No Runtime Context Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    assert!(run.runtime_context.is_none());

    let claimed = state
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .expect("claim run");
    assert!(claimed.runtime_context.is_none());

    crate::automation_v2::executor::run_automation_v2_run(state.clone(), claimed).await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Completed);
    assert_eq!(
        persisted.detail.as_deref(),
        Some("automation run completed")
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn automation_agent_templates_fall_back_to_global_workspace_library() {
    let state = ready_test_state().await;
    let global_workspace_root = state.workspace_index.snapshot().await.root;
    state
        .agent_teams
        .upsert_template(
            &global_workspace_root,
            tandem_orchestrator::AgentTemplate {
                template_id: "shared-copywriter".to_string(),
                display_name: Some("Shared Copywriter".to_string()),
                avatar_url: None,
                role: tandem_orchestrator::AgentRole::Worker,
                system_prompt: Some("You own messaging and release notes.".to_string()),
                default_model: None,
                skills: Vec::new(),
                default_budget: tandem_orchestrator::BudgetLimit::default(),
                capabilities: tandem_orchestrator::CapabilitySpec::default(),
            },
        )
        .await
        .expect("template upsert");

    let alternate_workspace = std::env::temp_dir().join(format!(
        "tandem-automation-template-fallback-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&alternate_workspace).expect("alternate workspace");
    let alternate_workspace_root = alternate_workspace.to_string_lossy().to_string();

    let resolved = crate::app::state::automation::resolve_automation_agent_template(
        &state,
        &alternate_workspace_root,
        "shared-copywriter",
    )
    .await
    .expect("resolve template")
    .expect("fallback template");

    assert_eq!(resolved.template_id, "shared-copywriter");
    assert_eq!(resolved.display_name.as_deref(), Some("Shared Copywriter"));

    let _ = std::fs::remove_dir_all(&alternate_workspace);
}

#[tokio::test]
async fn automation_agent_model_falls_back_to_effective_config_default() {
    let state = ready_test_state().await;
    state
        .config
        .patch_project(json!({
            "default_provider": "openai",
            "providers": {
                "openai": {
                    "default_model": "gpt-5-mini"
                }
            }
        }))
        .await
        .expect("patch config");

    let agent = AutomationAgentProfile {
        agent_id: "agent".to_string(),
        template_id: Some("shared-copywriter".to_string()),
        display_name: "Agent".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };
    let template = tandem_orchestrator::AgentTemplate {
        template_id: "shared-copywriter".to_string(),
        display_name: Some("Shared Copywriter".to_string()),
        avatar_url: None,
        role: tandem_orchestrator::AgentRole::Worker,
        system_prompt: Some("You own messaging and release notes.".to_string()),
        default_model: None,
        skills: Vec::new(),
        default_budget: tandem_orchestrator::BudgetLimit::default(),
        capabilities: tandem_orchestrator::CapabilitySpec::default(),
    };

    let resolved = crate::app::state::automation::resolve_automation_agent_model(
        &state,
        &agent,
        Some(&template),
    )
    .await
    .expect("resolved model");

    assert_eq!(resolved.provider_id, "openai");
    assert_eq!(resolved.model_id, "gpt-5-mini");
}

#[tokio::test]
async fn automation_run_rejects_invalid_activation_validation_snapshot() {
    let automation = AutomationV2Spec {
        automation_id: "auto-activation-validation-test".to_string(),
        name: "Activation Validation Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(".".to_string()),
        metadata: Some(json!({
            "context_materialization": {
                "routines": [
                    {
                        "routine_id": "collect_inputs",
                        "visible_context_objects": [],
                        "step_context_bindings": []
                    }
                ]
            },
            "plan_package_validation": {
                "ready_for_apply": false,
                "ready_for_activation": false,
                "blocker_count": 1,
                "warning_count": 0,
                "validation_state": {},
                "issues": [
                    {
                        "code": "cross_routine_scope_overlap",
                        "severity": "error",
                        "path": "routines[0]",
                        "message": "scope leak",
                        "blocking": true
                    }
                ]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();

    crate::automation_v2::executor::run_automation_v2_run(state.clone(), run).await;

    let persisted = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Failed);
    assert_eq!(
        persisted.detail.as_deref(),
        Some("plan package not ready for activation: scope leak (cross_routine_scope_overlap)")
    );
}

#[tokio::test]
async fn stale_running_automation_runs_are_paused_and_release_scheduler_capacity() {
    let automation = AutomationV2Spec {
        automation_id: "auto-stale-run-test".to_string(),
        name: "Stale Run Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp/stale-run-workspace".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();
    let claimed = state
        .claim_specific_automation_v2_run(&run_id)
        .await
        .expect("claim run");
    assert_eq!(claimed.status, AutomationRunStatus::Running);
    let session_id = "session-stale-run-test";
    let cancellation = state.cancellations.create(session_id).await;
    state
        .add_automation_v2_session(&run_id, session_id)
        .await
        .expect("attach session");
    state
        .set_automation_v2_session_mcp_servers(session_id, vec!["server-a".to_string()])
        .await;
    {
        let scheduler = state.automation_scheduler.read().await;
        assert_eq!(scheduler.active_count(), 1);
    }
    {
        let mut guard = state.automation_v2_runs.write().await;
        let persisted = guard.get_mut(&run_id).expect("persisted run");
        persisted.checkpoint.lifecycle_history.push(
            crate::automation_v2::types::AutomationLifecycleRecord {
                event: "run_started".to_string(),
                recorded_at_ms: now_ms().saturating_sub(180_000),
                reason: None,
                stop_kind: None,
                metadata: None,
            },
        );
    }

    let reaped = state.reap_stale_running_automation_runs(120_000).await;
    assert_eq!(reaped, 1);

    let persisted = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Paused);
    assert_eq!(
        persisted.pause_reason.as_deref(),
        Some("stale_no_provider_activity")
    );
    assert_eq!(persisted.stop_kind, Some(AutomationStopKind::StaleReaped));
    assert_eq!(
        persisted.detail.as_deref(),
        Some("automation run paused after no provider activity for at least 120s")
    );
    assert!(persisted.active_session_ids.is_empty());
    assert!(persisted.latest_session_id.is_none());
    assert!(cancellation.is_cancelled());
    assert!(state
        .automation_v2_session_runs
        .read()
        .await
        .get(session_id)
        .is_none());
    assert!(state
        .automation_v2_session_mcp_servers
        .read()
        .await
        .get(session_id)
        .is_none());
    {
        let scheduler = state.automation_scheduler.read().await;
        assert_eq!(scheduler.active_count(), 0);
    }
}

#[tokio::test]
async fn stale_running_automation_runs_mark_in_progress_nodes_as_repairable() {
    let automation = AutomationV2Spec {
        automation_id: "auto-stale-run-repairable-test".to_string(),
        name: "Stale Run Repairable Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec {
            nodes: vec![AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "cluster_topics".to_string(),
                agent_id: "writer".to_string(),
                objective: "Cluster the findings".to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: None,
                retry_policy: None,
                timeout_ms: None,
                max_tool_calls: None,
                stage_kind: None,
                gate: None,
                metadata: None,
            }],
        },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp/stale-run-repairable-workspace".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();
    state
        .claim_specific_automation_v2_run(&run_id)
        .await
        .expect("claim run");
    let session_id = "session-stale-run-repairable-test";
    let cancellation = state.cancellations.create(session_id).await;
    state
        .add_automation_v2_session(&run_id, session_id)
        .await
        .expect("attach session");
    {
        let mut guard = state.automation_v2_runs.write().await;
        let persisted = guard.get_mut(&run_id).expect("persisted run");
        persisted.checkpoint.pending_nodes = vec!["cluster_topics".to_string()];
        persisted
            .checkpoint
            .node_attempts
            .insert("cluster_topics".to_string(), 1);
        persisted.checkpoint.lifecycle_history.push(
            crate::automation_v2::types::AutomationLifecycleRecord {
                event: "run_started".to_string(),
                recorded_at_ms: now_ms().saturating_sub(180_000),
                reason: None,
                stop_kind: None,
                metadata: None,
            },
        );
        persisted.checkpoint.lifecycle_history.push(
            crate::automation_v2::types::AutomationLifecycleRecord {
                event: "node_started".to_string(),
                recorded_at_ms: now_ms().saturating_sub(180_000),
                reason: Some("node `cluster_topics` started".to_string()),
                stop_kind: None,
                metadata: Some(json!({
                    "node_id": "cluster_topics",
                    "attempt": 1,
                })),
            },
        );
    }

    let reaped = state.reap_stale_running_automation_runs(120_000).await;
    assert_eq!(reaped, 1);

    let persisted = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Paused);
    assert!(persisted
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("repairable node(s): cluster_topics")));
    let output = persisted
        .checkpoint
        .node_outputs
        .get("cluster_topics")
        .expect("repairable output");
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("needs_repair")
    );
    assert!(output
        .get("blocked_reason")
        .and_then(Value::as_str)
        .is_some_and(|reason| reason.contains("no provider activity")));
    assert_eq!(
        persisted
            .checkpoint
            .last_failure
            .as_ref()
            .map(|failure| failure.node_id.as_str()),
        Some("cluster_topics")
    );
    assert!(cancellation.is_cancelled());
}

#[tokio::test]
async fn stale_running_automation_runs_ignore_recent_session_activity() {
    let automation = AutomationV2Spec {
        automation_id: "auto-stale-session-activity-test".to_string(),
        name: "Stale Session Activity Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp/stale-session-activity-workspace".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();
    state
        .claim_specific_automation_v2_run(&run_id)
        .await
        .expect("claim run");
    let session_id = "session-stale-session-activity-test";
    let mut session = Session::new(Some("recent session activity".to_string()), None);
    session.id = session_id.to_string();
    session.time.updated = chrono::Utc::now();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");
    let cancellation = state.cancellations.create(session_id).await;
    state
        .add_automation_v2_session(&run_id, session_id)
        .await
        .expect("attach session");
    {
        let mut guard = state.automation_v2_runs.write().await;
        let persisted = guard.get_mut(&run_id).expect("persisted run");
        persisted.checkpoint.lifecycle_history.push(
            crate::automation_v2::types::AutomationLifecycleRecord {
                event: "run_started".to_string(),
                recorded_at_ms: now_ms().saturating_sub(180_000),
                reason: None,
                stop_kind: None,
                metadata: None,
            },
        );
    }

    let reaped = state.reap_stale_running_automation_runs(120_000).await;
    assert_eq!(reaped, 0);

    let persisted = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Running);
    assert_eq!(persisted.active_session_ids, vec![session_id.to_string()]);
    assert!(!cancellation.is_cancelled());
}

#[tokio::test]
async fn recover_in_flight_runs_does_not_relock_workspace_for_paused_runs() {
    let workspace_root = "/tmp/paused-workspace-lock-recovery".to_string();
    let automation = AutomationV2Spec {
        automation_id: "auto-paused-workspace-lock-recovery".to_string(),
        name: "Paused Workspace Lock Recovery".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(2),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(workspace_root.clone()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let state = ready_test_state().await;
    let paused_run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create paused run");
    let queued_run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create queued run");

    state
        .claim_specific_automation_v2_run(&paused_run.run_id)
        .await
        .expect("claim paused run");
    state
        .update_automation_v2_run(&paused_run.run_id, |row| {
            row.status = AutomationRunStatus::Paused;
            row.pause_reason = Some("paused for restart test".to_string());
            row.active_session_ids.clear();
            row.active_instance_ids.clear();
        })
        .await
        .expect("pause run");

    {
        let scheduler = state.automation_scheduler.read().await;
        assert!(!scheduler.locked_workspaces.contains_key(&workspace_root));
    }

    let recovered = state.recover_in_flight_runs().await;
    assert_eq!(recovered, 0);

    {
        let scheduler = state.automation_scheduler.read().await;
        assert!(!scheduler.locked_workspaces.contains_key(&workspace_root));
        assert!(scheduler
            .can_admit(&queued_run.run_id, Some(&workspace_root), &[])
            .is_ok());
    }
}

#[tokio::test]
async fn automation_node_prompt_timeout_cancels_the_session() {
    let state = ready_test_state().await;
    let session_id = "session-automation-timeout-test";
    let cancellation = state.cancellations.create(session_id).await;
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "timeout_node".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Timeout test".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        retry_policy: None,
        timeout_ms: Some(1),
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    };

    let error = crate::app::state::automation::run_automation_node_prompt_with_timeout(
        &state,
        session_id,
        &node,
        std::future::pending::<anyhow::Result<()>>(),
    )
    .await
    .expect_err("timeout should fail");

    assert!(error
        .to_string()
        .contains("automation node `timeout_node` timed out after 1 ms"));
    assert!(cancellation.is_cancelled());
}

#[tokio::test]
async fn automation_v2_approved_plan_materialization_is_recovered_from_snapshot() {
    let automation = AutomationV2Spec {
        automation_id: "auto-approved-plan-test".to_string(),
        name: "Approved Plan Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(".".to_string()),
        metadata: Some(json!({
            "plan_package_bundle": {
                "scope_snapshot": {
                    "plan_id": "plan-approved-1",
                    "plan_revision": 4,
                    "context_objects": [
                        {
                            "context_object_id": "ctx:plan:goal",
                            "name": "Plan goal",
                            "kind": "mission_goal",
                            "scope": "mission",
                            "owner_routine_id": "routine_a",
                            "declared_consumers": ["routine_a"],
                            "data_scope_refs": ["mission.goal"],
                            "validation_status": "pending",
                            "provenance": {
                                "plan_id": "plan-approved-1",
                                "routine_id": "routine_a"
                            },
                            "summary": "Plan goal"
                        }
                    ],
                    "credential_envelopes": []
                }
            },
            "approved_plan_materialization": {
                "plan_id": "plan-approved-1",
                "plan_revision": 4,
                "lifecycle_state": "approved",
                "routine_count": 1,
                "step_count": 1,
                "context_object_count": 1,
                "routines": [
                    {
                        "routine_id": "routine_a",
                        "step_ids": ["step_a"],
                        "visible_context_object_ids": ["ctx:plan:goal"],
                        "step_context_bindings": [
                            {
                                "step_id": "step_a",
                                "context_reads": ["ctx:plan:goal"],
                                "context_writes": []
                            }
                        ]
                    }
                ]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };

    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let runtime_context = state
        .automation_v2_runtime_context(&run)
        .expect("runtime context from approved plan");
    let snapshot = state
        .automation_v2_approved_plan_materialization(&run)
        .expect("approved plan materialization");
    assert_eq!(snapshot.plan_id, "plan-approved-1");
    assert_eq!(snapshot.plan_revision, 4);
    assert_eq!(snapshot.routine_count, 1);
    assert_eq!(snapshot.step_count, 1);
    assert_eq!(runtime_context.routines.len(), 1);
    assert_eq!(
        runtime_context.routines[0].visible_context_objects[0].context_object_id,
        "ctx:plan:goal"
    );
    assert_eq!(
        runtime_context.routines[0].step_context_bindings[0].step_id,
        "step_a"
    );
    assert_eq!(
        automation
            .approved_plan_materialization()
            .as_ref()
            .map(|materialization| materialization.plan_id.as_str()),
        Some("plan-approved-1")
    );
}

#[tokio::test]
async fn automation_runtime_context_merges_shared_context_packs() {
    let state = ready_test_state().await;
    let pack_id = format!("context-pack-{}", uuid::Uuid::new_v4());
    let shared_context = json!({
        "routines": [
            {
                "routine_id": "shared_routine",
                "visible_context_objects": [
                    {
                        "context_object_id": "ctx:shared:goal",
                        "name": "Shared goal",
                        "kind": "mission_goal",
                        "scope": "mission",
                        "owner_routine_id": "shared_routine",
                        "declared_consumers": ["shared_routine"],
                        "data_scope_refs": ["mission.goal"],
                        "validation_status": "pending",
                        "provenance": {
                            "plan_id": "plan-shared-1",
                            "routine_id": "shared_routine"
                        },
                        "summary": "Shared goal"
                    }
                ],
                "step_context_bindings": [
                    {
                        "step_id": "shared_step",
                        "context_reads": ["ctx:shared:goal"],
                        "context_writes": []
                    }
                ]
            }
        ]
    });
    state
        .put_context_pack(crate::http::context_packs::ContextPackRecord {
            pack_id: pack_id.clone(),
            title: "Shared workflow context".to_string(),
            summary: Some("Shared runtime context".to_string()),
            project_key: Some("project-a".to_string()),
            allowed_project_keys: Vec::new(),
            workspace_root: ".".to_string(),
            source_plan_id: Some("plan-shared-1".to_string()),
            source_automation_id: None,
            source_run_id: None,
            source_context_run_id: None,
            visibility_scope: crate::http::context_packs::ContextPackVisibilityScope::SameProject,
            state: crate::http::context_packs::ContextPackState::Published,
            manifest: crate::http::context_packs::ContextPackManifest {
                runtime_context: Some(shared_context),
                ..Default::default()
            },
            bindings: Vec::new(),
            freshness_window_hours: None,
            published_actor_metadata: None,
            revoked_actor_metadata: None,
            superseded_actor_metadata: None,
            superseded_by_pack_id: None,
            published_at_ms: Some(1),
            revoked_at_ms: None,
            superseded_at_ms: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        })
        .await
        .expect("store pack");

    let automation = AutomationV2Spec {
        automation_id: "automation-shared-context".to_string(),
        name: "Shared Context".to_string(),
        description: None,
        status: AutomationV2Status::Draft,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(".".to_string()),
        metadata: Some(json!({
            "shared_context_bindings": [
                { "pack_id": pack_id, "required": true }
            ]
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };

    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let runtime_context = run.runtime_context.expect("runtime context");
    assert_eq!(runtime_context.routines.len(), 1);
    assert_eq!(runtime_context.routines[0].routine_id, "shared_routine");
    assert_eq!(
        runtime_context.routines[0].visible_context_objects[0].context_object_id,
        "ctx:shared:goal"
    );
    assert_eq!(
        runtime_context.routines[0].step_context_bindings[0].step_id,
        "shared_step"
    );
}

#[test]
fn first_attempt_structured_json_prompt_without_output_path_requires_handoff_even_without_enforcement(
) {
    let automation = AutomationV2Spec {
        automation_id: "automation-structured-defaults".to_string(),
        name: "Structured Handoff Defaults".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
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
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "discover".to_string(),
        agent_id: "research-discover".to_string(),
        objective: "Enumerate sources".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "prompt": "Enumerate the workspace and identify source files."
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "research-discover".to_string(),
        template_id: None,
        display_name: "Research Discover".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["glob".to_string(), "read".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-structured-defaults",
        &node,
        1,
        &agent,
        &[],
        &["glob".to_string(), "read".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Structured Handoff Expectation"));
    assert!(prompt.contains("`status` set to `completed`"));
    assert!(prompt.contains(
            "Do not claim success unless the required structured handoff was actually returned in the final response."
        ));
}

#[test]
fn report_generation_objective_does_not_imply_email_delivery_execution() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Draft the report in simple HTML suitable for email body delivery.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/generate-report.html"
            }
        })),
    };

    assert!(!crate::app::state::automation::automation_node_requires_email_delivery(&node));
}

#[test]
fn execute_goal_objective_with_gmail_draft_or_send_requires_email_delivery() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "operator".to_string(),
        objective: "Create a Gmail draft or send the final HTML summary email to recipient@example.com if mail tools are available.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "approval_gate".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    };

    assert!(crate::app::state::automation::automation_node_requires_email_delivery(&node));
}

#[test]
fn email_delivery_status_uses_recipient_from_objective_when_metadata_missing() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "operator".to_string(),
        objective: "Create a Gmail draft or send the final HTML summary email to recipient@example.com if mail tools are available.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "approval_gate".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    };

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "A Gmail draft has been created.\n\n{\"status\":\"completed\",\"approved\":true}",
            None,
            &json!({
                "requested_tools": ["glob", "read", "mcp_list"],
                "executed_tools": ["read", "glob", "mcp_list"],
                "tool_call_counts": {"read": 1, "glob": 1, "mcp_list": 1},
                "workspace_inspection_used": true,
                "email_delivery_attempted": false,
                "email_delivery_succeeded": false,
                "latest_email_delivery_failure": null,
                "capability_resolution": {
                    "email_tool_diagnostics": {
                        "available_tools": ["mcp.composio_1.gmail_send_email", "mcp.composio_1.gmail_create_email_draft"],
                        "offered_tools": ["mcp.composio_1.gmail_send_email", "mcp.composio_1.gmail_create_email_draft"],
                        "available_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_draft_tools": ["mcp.composio_1.gmail_create_email_draft"],
                        "offered_draft_tools": ["mcp.composio_1.gmail_create_email_draft"]
                    }
                }
            }),
            None,
        );

    assert_eq!(status, "blocked");
    assert_eq!(
        reason.as_deref(),
        Some(
            "email delivery to `recipient@example.com` was requested but no email draft/send tool executed"
        )
    );
    assert_eq!(approved, Some(true));
}

#[test]
fn research_workflow_failure_kind_detects_missing_citations() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let artifact_validation = json!({
        "semantic_block_reason": "research completed without citation-backed claims",
        "unmet_requirements": ["citations_missing", "web_sources_reviewed_missing"],
        "verification": {
            "verification_failed": false
        }
    });

    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            "blocked",
            None,
            Some("research completed without citation-backed claims"),
            Some(&artifact_validation),
        )
        .as_deref(),
        Some("research_citations_missing")
    );
    assert_eq!(
        detect_automation_node_phase(&node, "blocked", Some(&artifact_validation)),
        "research_validation"
    );
}

#[test]
fn research_workflow_defaults_to_warning_without_strict_source_coverage() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true,
                "allow_preexisting_output_reuse": true
            }
        })),
    };
    let artifact_validation = json!({
        "unmet_requirements": ["no_concrete_reads", "citations_missing", "missing_successful_web_research"],
        "verification": {
            "verification_failed": false
        }
    });

    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            "completed",
            None,
            None,
            Some(&artifact_validation)
        ),
        None
    );
    assert_eq!(
        detect_automation_node_phase(&node, "completed", Some(&artifact_validation)),
        "completed"
    );
}

#[test]
fn validator_summary_reports_repair_attempt_state() {
    let artifact_validation = json!({
        "semantic_block_reason": "research completed without citation-backed claims",
        "unmet_requirements": ["citations_missing"],
        "repair_attempted": true,
        "repair_attempt": 2,
        "repair_attempts_remaining": 0,
        "repair_succeeded": false,
        "repair_exhausted": true,
    });
    let summary = build_automation_validator_summary(
        crate::AutomationOutputValidatorKind::ResearchBrief,
        "blocked",
        Some("research completed without citation-backed claims"),
        Some(&artifact_validation),
    );
    assert!(summary.repair_attempted);
    assert_eq!(summary.repair_attempt, 2);
    assert_eq!(summary.repair_attempts_remaining, 0);
    assert!(!summary.repair_succeeded);
    assert!(summary.repair_exhausted);
}

#[test]
fn artifact_validation_uses_structured_repair_exhaustion_state_from_session_text() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-repair-state-test-{}", now_ms()));
    std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
    std::fs::write(workspace_root.join("inputs/questions.md"), "Question")
        .expect("seed input file");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let mut session = Session::new(Some("research repair exhausted".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path":"marketing-brief.md",
                "content":"# Marketing Brief\n\n## Findings\nBlocked draft without citations.\n"
            }),
            result: Some(json!({"output":"written"})),
            error: None,
        }],
    ));
    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &[
            "glob".to_string(),
            "read".to_string(),
            "websearch".to_string(),
            "write".to_string(),
        ],
    );
    let session_text = r#"TOOL_MODE_REQUIRED_NOT_SATISFIED: PREWRITE_REQUIREMENTS_EXHAUSTED

{"status":"blocked","reason":"prewrite requirements exhausted before final artifact validation","failureCode":"PREWRITE_REQUIREMENTS_EXHAUSTED","repairAttempt":2,"repairAttemptsRemaining":0,"repairExhausted":true,"unmetRequirements":["concrete_read_required","successful_web_research_required"]}"#;
    let (_accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        session_text,
        &tool_telemetry,
        None,
        Some((
            "marketing-brief.md".to_string(),
            "# Marketing Brief\n\n## Findings\nBlocked draft without citations.\n".to_string(),
        )),
        &std::collections::BTreeSet::new(),
    );
    assert!(rejected.is_some());
    assert_eq!(
        metadata.get("repair_attempt").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        metadata
            .get("repair_attempts_remaining")
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        metadata.get("repair_exhausted").and_then(Value::as_bool),
        Some(true)
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn research_artifact_validation_requires_citations_and_web_sources_reviewed() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-research-citation-test-{}", now_ms()));
    std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
    std::fs::write(workspace_root.join("inputs/questions.md"), "Question")
        .expect("seed input file");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let mut session = Session::new(Some("research citations".to_string()), None);
    session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "read".to_string(),
                    args: json!({"path":"inputs/questions.md"}),
                    result: Some(json!({"output":"Question"})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "websearch".to_string(),
                    args: json!({"query":"market trends"}),
                    result: Some(json!({"output":"Search results found"})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path":"marketing-brief.md",
                        "content":"# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Files not reviewed\n- inputs/references.md: not available in this run.\n\n## Findings\nClaims are summarized here without explicit citations.\n"
                    }),
                    result: Some(json!({"output":"written"})),
                    error: None,
                },
            ],
        ));

    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &[
            "read".to_string(),
            "write".to_string(),
            "websearch".to_string(),
        ],
    );
    let (_, artifact_validation, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            "",
            &tool_telemetry,
            None,
            Some((
                "marketing-brief.md".to_string(),
                "# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Findings\nClaims are summarized here without explicit citations.\n".to_string(),
            )),
            &std::collections::BTreeSet::new(),
        );

    assert_eq!(
        rejected.as_deref(),
        Some("research completed without citation-backed claims")
    );
    assert_eq!(
        artifact_validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![json!("citations_missing")]
    );
    assert_eq!(
        artifact_validation
            .get("artifact_candidates")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|value| value.get("citation_count"))
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        artifact_validation
            .get("citation_count")
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        artifact_validation
            .get("web_sources_reviewed_present")
            .and_then(Value::as_bool),
        Some(false)
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn research_citations_validation_accepts_external_research_without_files_reviewed_section() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-research-sources-test-{}", now_ms()));
    std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
    std::fs::write(workspace_root.join("inputs/questions.md"), "Question")
        .expect("seed input file");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Research current web sources".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "citations".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return a citation handoff.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/research-sources.json",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let mut session = Session::new(Some("research sources".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path":"inputs/questions.md"}),
                result: Some(json!({"output":"Question"})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "websearch".to_string(),
                args: json!({"query":"autonomous AI agentic workflows 2024 2025"}),
                result: Some(json!({"output":"Search results found"})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path":".tandem/artifacts/research-sources.json",
                    "content":"# Research Sources\n\n## Summary\nCurrent external research was gathered successfully.\n\n## Citations\n1. AI Agents in 2025: Expectations vs. Reality | IBM. Source note: https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n2. Agentic AI, explained | MIT Sloan. Source note: https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n\n## Web sources reviewed\n- https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n- https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n"
                }),
                result: Some(json!({"output":"written"})),
                error: None,
            },
        ],
    ));

    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &[
            "read".to_string(),
            "write".to_string(),
            "websearch".to_string(),
        ],
    );
    let (_accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &tool_telemetry,
        None,
        Some((
            ".tandem/artifacts/research-sources.json".to_string(),
            "# Research Sources\n\n## Summary\nCurrent external research was gathered successfully.\n\n## Citations\n1. AI Agents in 2025: Expectations vs. Reality | IBM. Source note: https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n2. Agentic AI, explained | MIT Sloan. Source note: https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n\n## Web sources reviewed\n- https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n- https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n".to_string(),
        )),
        &std::collections::BTreeSet::new(),
    );

    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("files_reviewed_missing"))));
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("files_reviewed_not_backed_by_read"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}
