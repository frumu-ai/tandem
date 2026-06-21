#[tokio::test]
async fn automation_v2_run_history_lists_archived_blocked_runs() {
    let mut state = test_state_with_path(tmp_resource_file("automation-history-state"));
    state.automation_v2_runs_path = tmp_resource_file("automation-history-runs");
    let run = AutomationRunBuilder::new("run-history-blocked", "auto-history")
        .status(AutomationRunStatus::Blocked)
        .build();
    {
        let mut runs = state.automation_v2_runs.write().await;
        runs.insert(run.run_id.clone(), run.clone());
    }

    let archived = state
        .archive_stale_automation_v2_runs(0)
        .await
        .expect("archive stale runs");
    assert_eq!(archived, 1);
    assert!(state
        .automation_v2_runs
        .read()
        .await
        .get("run-history-blocked")
        .is_none());

    let rows = state.list_automation_v2_runs(None, 20).await;
    assert!(rows.iter().any(|row| {
        row.run_id == "run-history-blocked" && row.status == AutomationRunStatus::Blocked
    }));

    let filtered = state
        .list_automation_v2_runs(Some("auto-history"), 20)
        .await;
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].run_id, "run-history-blocked");
}

#[tokio::test]
async fn automation_v2_recovers_legacy_context_runs_for_history_and_library() {
    let root = std::env::temp_dir().join(format!(
        "tandem-context-run-recovery-{}",
        uuid::Uuid::new_v4()
    ));
    let shared_path = root.join("data").join("system").join("shared_resources.json");
    std::fs::create_dir_all(shared_path.parent().expect("shared parent")).expect("shared dir");
    let mut state = test_state_with_path(shared_path);
    state.automations_v2_path = root.join("data").join("automations_v2.json");
    state.automation_v2_runs_path = root.join("data").join("automation_v2_runs.json");

    let context_run_id = "automation-v2-automation-v2-run-context-history";
    let context_run_dir = root.join("context_runs").join(context_run_id);
    std::fs::create_dir_all(&context_run_dir).expect("context run dir");
    let run_state = json!({
        "run_id": context_run_id,
        "run_type": "automation_v2",
        "tenant_context": {
            "org_id": "local",
            "workspace_id": "local",
            "source": "local_implicit"
        },
        "source_client": "automation_v2_scheduler",
        "status": "blocked",
        "objective": "Recovered automation from context state",
        "workspace": {
            "workspace_id": "",
            "canonical_path": "/tmp/recovered-workspace",
            "lease_epoch": 0
        },
        "steps": [
            {"step_id": "inspect", "title": "Inspect repository", "status": "done"},
            {"step_id": "write", "title": "Write report", "status": "blocked"}
        ],
        "tasks": [
            {
                "payload": {
                    "receipt_timeline": {
                        "records": [
                            {
                                "payload": {
                                    "automation_id": "auto-from-context",
                                    "run_id": "automation-v2-run-context-history"
                                }
                            }
                        ]
                    }
                }
            }
        ],
        "created_at_ms": 10,
        "started_at_ms": 11,
        "updated_at_ms": 20
    });
    std::fs::write(
        context_run_dir.join("run_state.json"),
        serde_json::to_string_pretty(&run_state).expect("run state json"),
    )
    .expect("write context run state");

    let queued_context_run_id = "automation-v2-automation-v2-run-context-queued";
    let queued_context_run_dir = root.join("context_runs").join(queued_context_run_id);
    std::fs::create_dir_all(&queued_context_run_dir).expect("queued context run dir");
    let mut queued_run_state = run_state.clone();
    queued_run_state["run_id"] = json!(queued_context_run_id);
    queued_run_state["status"] = json!("queued");
    queued_run_state["tasks"][0]["payload"]["receipt_timeline"]["records"][0]["payload"]
        ["run_id"] = json!("automation-v2-run-context-queued");
    queued_run_state["tasks"][0]["payload"]["receipt_timeline"]["records"][0]["payload"]
        ["automation_id"] = json!("auto-from-queued-context");
    std::fs::write(
        queued_context_run_dir.join("run_state.json"),
        serde_json::to_string_pretty(&queued_run_state).expect("queued run state json"),
    )
    .expect("write queued context run state");

    let rows = state.list_automation_v2_runs(None, 20).await;
    assert!(
        rows.iter()
            .all(|row| row.run_id != "automation-v2-run-context-queued"),
        "non-terminal context-run mirrors should not be recovered as queued automation runs"
    );
    let recovered = rows
        .iter()
        .find(|row| row.run_id == "automation-v2-run-context-history")
        .expect("recovered context run listed");
    assert_eq!(recovered.automation_id, "auto-from-context");
    assert_eq!(recovered.status, AutomationRunStatus::Blocked);
    assert_eq!(recovered.checkpoint.completed_nodes, vec!["inspect"]);
    assert_eq!(recovered.checkpoint.blocked_nodes, vec!["write"]);

    let filtered = state
        .list_automation_v2_runs(Some("auto-from-context"), 20)
        .await;
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].run_id, "automation-v2-run-context-history");

    let detail = state
        .get_automation_v2_run("automation-v2-run-context-history")
        .await
        .expect("recovered context run detail");
    assert_eq!(detail.automation_id, "auto-from-context");
    assert_eq!(
        detail
            .automation_snapshot
            .as_ref()
            .map(|snapshot| snapshot.name.as_str()),
        Some("Recovered automation from context state")
    );

    state
        .load_automation_v2_runs()
        .await
        .expect("load context-recovered automation runs");
    assert!(state
        .automation_v2_runs
        .read()
        .await
        .contains_key("automation-v2-run-context-history"));

    let automations = state.list_automations_v2().await;
    let automation = automations
        .iter()
        .find(|row| row.automation_id == "auto-from-context")
        .expect("context run snapshot recovered as library definition");
    assert_eq!(automation.name, "Recovered automation from context state");
    assert_eq!(automation.workspace_root.as_deref(), Some("/tmp/recovered-workspace"));
    assert_eq!(automation.flow.nodes.len(), 2);
}

#[tokio::test]
async fn automation_v2_load_drops_nonterminal_recovered_context_runs() {
    let mut state = test_state_with_path(tmp_resource_file("automation-recovered-context-state"));
    state.automation_v2_runs_path = tmp_resource_file("automation-recovered-context-runs");
    let mut run = AutomationRunBuilder::new("run-recovered-context-queued", "auto-recovered")
        .status(AutomationRunStatus::Queued)
        .build();
    run.trigger_type = "recovered_context_run".to_string();
    run.automation_snapshot = Some(AutomationSpecBuilder::new("auto-recovered").build());
    {
        let mut runs = state.automation_v2_runs.write().await;
        runs.insert(run.run_id.clone(), run.clone());
    }
    state
        .persist_automation_v2_runs()
        .await
        .expect("persist recovered context run");
    let shard_path = automation_v2_run_history_shard_path(&state.automation_v2_runs_path, &run);
    std::fs::create_dir_all(shard_path.parent().expect("history shard parent"))
        .expect("history shard dir");
    std::fs::write(
        &shard_path,
        serde_json::to_string_pretty(&run).expect("history shard json"),
    )
    .expect("write stale history shard");
    state.automation_v2_runs.write().await.clear();

    state
        .load_automation_v2_runs()
        .await
        .expect("reload recovered context runs");

    assert!(state
        .automation_v2_runs
        .read()
        .await
        .get("run-recovered-context-queued")
        .is_none());
    assert!(state
        .get_automation_v2_run("run-recovered-context-queued")
        .await
        .is_none());
    assert!(state
        .list_automation_v2_runs(Some("auto-recovered"), 20)
        .await
        .is_empty());
}

#[tokio::test]
async fn automation_v2_context_recovery_does_not_replace_existing_definition() {
    let mut state = test_state_with_path(tmp_resource_file("automation-context-definition-state"));
    state.automations_v2_path = tmp_resource_file("automation-context-definition-defs");
    state.automation_v2_runs_path = tmp_resource_file("automation-context-definition-runs");

    let mut existing = AutomationSpecBuilder::new("auto-existing")
        .name("Real Automation")
        .metadata(json!({ "source": "real" }))
        .build();
    existing.updated_at_ms = 10;
    state
        .automations_v2
        .write()
        .await
        .insert(existing.automation_id.clone(), existing.clone());

    let mut recovered_snapshot = AutomationSpecBuilder::new("auto-existing")
        .name("Recovered Automation")
        .metadata(json!({ "recovered_from": "context_run" }))
        .build();
    recovered_snapshot.updated_at_ms = 20;
    let mut run = AutomationRunBuilder::new("run-recovered-definition", "auto-existing")
        .status(AutomationRunStatus::Blocked)
        .build();
    run.trigger_type = "recovered_context_run".to_string();
    run.automation_snapshot = Some(recovered_snapshot);
    {
        let mut runs = state.automation_v2_runs.write().await;
        runs.insert(run.run_id.clone(), run);
    }
    state
        .persist_automation_v2_runs()
        .await
        .expect("persist recovered context snapshot");
    state.automation_v2_runs.write().await.clear();

    state
        .load_automation_v2_runs()
        .await
        .expect("reload recovered context snapshot");

    let automation = state
        .get_automation_v2("auto-existing")
        .await
        .expect("existing automation");
    assert_eq!(automation.name, "Real Automation");
    assert_eq!(
        automation
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("source"))
            .and_then(Value::as_str),
        Some("real")
    );
}

#[tokio::test]
async fn automation_v2_run_update_hydrates_history_only_run() {
    let mut state = ready_test_state().await;
    state.automation_v2_runs_path = tmp_resource_file("automation-history-update-runs");
    let run = AutomationRunBuilder::new("run-history-update", "auto-history")
        .status(AutomationRunStatus::Blocked)
        .build();
    {
        let mut runs = state.automation_v2_runs.write().await;
        runs.clear();
        runs.insert(run.run_id.clone(), run.clone());
    }

    let archived = state
        .archive_stale_automation_v2_runs(0)
        .await
        .expect("archive stale runs");
    assert_eq!(archived, 1);
    assert!(state
        .automation_v2_runs
        .read()
        .await
        .get("run-history-update")
        .is_none());

    let updated = state
        .update_automation_v2_run("run-history-update", |row| {
            row.status = AutomationRunStatus::Cancelled;
            row.detail = Some("cancelled from history".to_string());
        })
        .await
        .expect("history-only run can be updated");

    assert_eq!(updated.status, AutomationRunStatus::Cancelled);
    assert_eq!(updated.detail.as_deref(), Some("cancelled from history"));
    assert!(state
        .automation_v2_runs
        .read()
        .await
        .get("run-history-update")
        .is_some());
}

#[tokio::test]
async fn automation_run_requeue_increments_attempt_counter() {
    let _guard = automation_executor_test_lock().lock().await;
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
                allowed_connections: Vec::new(),
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
                tool_policy: None,
                mcp_policy: None,
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
            profile: None,
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
    let _guard = automation_executor_test_lock().lock().await;
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
            profile: None,
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
    let _guard = automation_executor_test_lock().lock().await;
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
            profile: None,
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
            allowed_connections: Vec::new(),
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
    let mut automation = AutomationV2Spec {
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
            profile: None,
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
    let mut automation = AutomationV2Spec {
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
            profile: None,
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
    let tenant_context = TenantContext::explicit_user_workspace(
        "org-stale-recovery".to_string(),
        "workspace-stale-recovery".to_string(),
        Some("user-stale-recovery".to_string()),
        "test-suite".to_string(),
    );
    automation.set_tenant_context(&tenant_context);
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
    assert_eq!(persisted.tenant_context.org_id, "org-stale-recovery");
    assert_eq!(
        persisted.tenant_context.workspace_id,
        "workspace-stale-recovery"
    );
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
    let mut automation = AutomationV2Spec {
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
                tool_policy: None,
                mcp_policy: None,
                retry_policy: None,
                timeout_ms: Some(60_000),
                max_tool_calls: None,
                stage_kind: None,
                gate: None,
                metadata: None,
            }],
        },
        execution: AutomationExecutionPolicy {
            profile: None,
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
    let tenant_context = TenantContext::explicit_user_workspace(
        "org-stale-recovery".to_string(),
        "workspace-stale-recovery".to_string(),
        Some("user-stale-recovery".to_string()),
        "test-suite".to_string(),
    );
    automation.set_tenant_context(&tenant_context);
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

    let resumed = state.auto_resume_stale_reaped_runs().await;
    assert_eq!(resumed, 1);
    let resumed_run = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("resumed run");
    assert_eq!(resumed_run.tenant_context.org_id, "org-stale-recovery");
    assert_eq!(
        resumed_run.tenant_context.workspace_id,
        "workspace-stale-recovery"
    );
    assert_eq!(resumed_run.status, AutomationRunStatus::Queued);
    assert_eq!(resumed_run.pause_reason, None);
    assert_eq!(resumed_run.stop_kind, None);
    assert!(resumed_run
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "cluster_topics"));
    assert_eq!(
        resumed_run
            .checkpoint
            .node_outputs
            .get("cluster_topics")
            .and_then(|output| output.get("status"))
            .and_then(Value::as_str),
        Some("needs_repair")
    );
    assert!(resumed_run
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|entry| entry.event == "run_auto_resumed"));
}


#[tokio::test]
async fn guardrail_stopped_run_auto_resumes_after_quota_override_approval() {
    let agent_id = "agent-guardrail-resume";
    let automation = AutomationV2Spec {
        automation_id: "auto-guardrail-resume".to_string(),
        name: "Guardrail Resume Test".to_string(),
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
        agents: vec![AutomationAgentProfile {
            agent_id: agent_id.to_string(),
            template_id: None,
            display_name: "Guardrail Agent".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: AutomationAgentToolPolicy {
                allowlist: vec!["*".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
                allowed_connections: Vec::new(),
            },
            approval_policy: None,
        }],
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            profile: None,
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "creator-guardrail-resume".to_string(),
        workspace_root: Some("/tmp/guardrail-resume-workspace".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let state = ready_test_state().await;
    state.put_automation_v2(automation.clone()).await.expect("put automation");
    let mut run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();
    run.status = AutomationRunStatus::Paused;
    run.detail = Some("weekly spend cap exceeded for agent-guardrail-resume".to_string());
    run.pause_reason = run.detail.clone();
    run.stop_kind = Some(AutomationStopKind::GuardrailStopped);
    run.stop_reason = run.detail.clone();
    run.automation_snapshot = Some(automation);
    {
        let mut runs = state.automation_v2_runs.write().await;
        runs.insert(run_id.clone(), run);
    }

    assert_eq!(state.auto_resume_stale_reaped_runs().await, 0);
    assert_eq!(
        state
            .get_automation_v2_run(&run_id)
            .await
            .expect("paused run")
            .status,
        AutomationRunStatus::Paused
    );

    let now = now_ms();
    {
        let mut governance = state.automation_governance.write().await;
        governance.approvals.insert(
            "approval-guardrail-resume".to_string(),
            crate::automation_v2::governance::GovernanceApprovalRequest {
                approval_id: "approval-guardrail-resume".to_string(),
                request_type: crate::automation_v2::governance::GovernanceApprovalRequestType::QuotaOverride,
                requested_by: crate::automation_v2::governance::GovernanceActorRef {
                    kind: crate::automation_v2::governance::GovernanceActorKind::Human,
                    actor_id: Some("reviewer".to_string()),
                    source: Some("test".to_string()),
                },
                target_resource: crate::automation_v2::governance::GovernanceResourceRef {
                    resource_type: "agent".to_string(),
                    id: agent_id.to_string(),
                },
                rationale: "allow guarded resume".to_string(),
                context: serde_json::json!({}),
                status: crate::automation_v2::governance::GovernanceApprovalStatus::Approved,
                expires_at_ms: now + 60_000,
                tenant_context: None,
                reviewed_by: Some(crate::automation_v2::governance::GovernanceActorRef {
                    kind: crate::automation_v2::governance::GovernanceActorKind::Human,
                    actor_id: Some("reviewer".to_string()),
                    source: Some("test".to_string()),
                }),
                reviewed_at_ms: Some(now),
                review_notes: Some("approved".to_string()),
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
    }

    assert_eq!(state.auto_resume_stale_reaped_runs().await, 1);
    let resumed = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("resumed run");
    assert_eq!(resumed.status, AutomationRunStatus::Queued);
    assert_eq!(resumed.pause_reason, None);
    assert_eq!(resumed.stop_kind, None);
    assert_eq!(resumed.stop_reason, None);
    assert!(resumed
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|entry| entry.event == "run_auto_resumed"
            && entry.reason.as_deref() == Some("auto_resume_after_guardrail_override")));
}

#[tokio::test]
async fn awaiting_approval_runs_are_marked_stale_with_visible_manual_policy() {
    let automation = AutomationV2Spec {
        automation_id: "auto-awaiting-approval-stale".to_string(),
        name: "Awaiting Approval Stale Test".to_string(),
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
            profile: None,
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
        workspace_root: Some("/tmp/awaiting-approval-stale-workspace".to_string()),
        metadata: None,
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
        .expect("put automation");
    let mut run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();
    run.status = AutomationRunStatus::AwaitingApproval;
    run.detail = Some("awaiting approval for gate `approval`".to_string());
    run.checkpoint.awaiting_gate = Some(AutomationPendingGate {
        node_id: "approval".to_string(),
        title: "Publish approval".to_string(),
        instructions: Some("Approve before publishing.".to_string()),
        decisions: vec!["approve".to_string(), "cancel".to_string()],
        rework_targets: Vec::new(),
        requested_at_ms: now_ms().saturating_sub(2 * 24 * 60 * 60 * 1000),
        upstream_node_ids: Vec::new(),
        metadata: None,
    });
    {
        let mut runs = state.automation_v2_runs.write().await;
        runs.insert(run_id.clone(), run);
    }

    assert_eq!(state.mark_stale_awaiting_approval_runs().await, 1);
    assert_eq!(state.mark_stale_awaiting_approval_runs().await, 0);

    let updated = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("updated run");
    assert_eq!(updated.status, AutomationRunStatus::AwaitingApproval);
    assert!(updated
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("awaiting manual approval")));
    let metadata = updated
        .checkpoint
        .awaiting_gate
        .as_ref()
        .and_then(|gate| gate.metadata.as_ref())
        .expect("stale gate metadata");
    assert_eq!(metadata.get("stale").and_then(Value::as_bool), Some(true));
    assert_eq!(
        metadata.get("stale_policy").and_then(Value::as_str),
        Some("manual_only_visible_status")
    );
    assert!(updated
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|entry| entry.event == "approval_gate_marked_stale"));
}

#[tokio::test]
async fn stale_running_automation_runs_fail_terminal_in_progress_nodes() {
    let automation = AutomationV2Spec {
        automation_id: "auto-stale-run-terminal-test".to_string(),
        name: "Stale Run Terminal Test".to_string(),
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
                tool_policy: None,
                mcp_policy: None,
                retry_policy: Some(json!({"max_attempts": 1})),
                timeout_ms: Some(60_000),
                max_tool_calls: None,
                stage_kind: None,
                gate: None,
                metadata: None,
            }],
        },
        execution: AutomationExecutionPolicy {
            profile: None,
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
        workspace_root: Some("/tmp/stale-run-terminal-workspace".to_string()),
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
    state
        .add_automation_v2_session(&run_id, "session-stale-run-terminal-test")
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
    assert_eq!(persisted.status, AutomationRunStatus::Failed);
    assert_eq!(persisted.pause_reason, None);
    assert!(persisted
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("terminal stale node(s): cluster_topics")));
    let output = persisted
        .checkpoint
        .node_outputs
        .get("cluster_topics")
        .expect("terminal output");
    assert_eq!(output.get("status").and_then(Value::as_str), Some("failed"));
    assert_eq!(
        output.get("failure_kind").and_then(Value::as_str),
        Some("run_failed")
    );
    assert!(persisted
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|record| record.event == "run_failed_stale_no_provider_activity"));
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
            profile: None,
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


// --- TAN-214: golden tests for the email approval workflow ------------------
//
// The flagship contract: an agent composes an email, the run pauses at a
// HumanApprovalGate, and the send node cannot complete before an approve
// decision — and never completes after cancel. These tests pin the gate
// state machine (pause_automation_run_for_gate / apply_automation_gate_decision)
// at the checkpoint level: "send executed" == the send node leaving
// pending_nodes for completed_nodes.

fn email_flow_node(node_id: &str, objective: &str, depends_on: Vec<String>) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "mailer".to_string(),
        objective: objective.to_string(),
        depends_on,
        input_refs: Vec::new(),
        output_contract: None,
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    }
}

fn email_approval_automation(automation_id: &str) -> AutomationV2Spec {
    AutomationV2Spec {
        automation_id: automation_id.to_string(),
        name: "Email approval golden".to_string(),
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
            nodes: vec![
                email_flow_node("compose_email", "Draft the email", Vec::new()),
                email_flow_node(
                    "approval_gate",
                    "Human review",
                    vec!["compose_email".to_string()],
                ),
                email_flow_node("send_email", "Send the email", vec!["approval_gate".to_string()]),
            ],
        },
        execution: AutomationExecutionPolicy {
            profile: None,
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
        workspace_root: Some("/tmp/email-approval-golden".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

fn email_pending_gate() -> AutomationPendingGate {
    AutomationPendingGate {
        node_id: "approval_gate".to_string(),
        title: "Review the drafted email".to_string(),
        instructions: Some("Approve before the email is sent.".to_string()),
        decisions: vec![
            "approve".to_string(),
            "rework".to_string(),
            "cancel".to_string(),
        ],
        rework_targets: vec!["compose_email".to_string()],
        requested_at_ms: now_ms(),
        upstream_node_ids: vec!["compose_email".to_string()],
        metadata: None,
    }
}

/// Build a run paused at the approval gate: compose completed with a draft,
/// gate + send still pending.
async fn paused_email_run(
    state: &crate::AppState,
    automation: &AutomationV2Spec,
) -> AutomationV2RunRecord {
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("put automation");
    let mut run = state
        .create_automation_v2_run(automation, "manual")
        .await
        .expect("create run");
    run.checkpoint.completed_nodes = vec!["compose_email".to_string()];
    run.checkpoint.pending_nodes = vec!["approval_gate".to_string(), "send_email".to_string()];
    run.checkpoint.node_outputs.insert(
        "compose_email".to_string(),
        json!({
            "contract_kind": "email_draft",
            "summary": "Drafted email to customer",
            "content": { "to": "customer@example.com", "subject": "Update", "body": "Hello" },
        }),
    );
    crate::app::state::pause_automation_run_for_gate(
        &mut run,
        email_pending_gate(),
        vec!["send_email".to_string()],
    );
    run
}

fn send_email_completed(run: &AutomationV2RunRecord) -> bool {
    run.checkpoint
        .completed_nodes
        .iter()
        .any(|node| node == "send_email")
}

fn human_reviewer() -> crate::automation_v2::governance::GovernanceActorRef {
    crate::automation_v2::governance::GovernanceActorRef::human(
        Some("reviewer-1".to_string()),
        "control_panel",
    )
}

#[tokio::test]
async fn email_approval_golden_approve_path() {
    let state = ready_test_state().await;
    let automation = email_approval_automation("auto-email-approve");
    let mut run = paused_email_run(&state, &automation).await;

    // Golden pre-approval state: paused, send not executed, gate visible.
    assert_eq!(run.status, AutomationRunStatus::AwaitingApproval);
    assert!(!send_email_completed(&run), "send must not run before approval");
    assert_eq!(
        run.checkpoint.awaiting_gate.as_ref().map(|g| g.node_id.as_str()),
        Some("approval_gate")
    );
    assert!(run.checkpoint.gate_history.is_empty());

    let gate = email_pending_gate();
    let outcome = crate::app::state::apply_automation_gate_decision(
        &mut run,
        &automation,
        &gate,
        "approve",
        Some("LGTM".to_string()),
        Some(human_reviewer()),
    );
    assert!(matches!(
        outcome,
        crate::app::state::AutomationGateDecisionOutcome::Applied
    ));

    // Golden post-approval state: queued to continue, gate recorded once with
    // the human actor, gate node completed with an approval_gate output, and
    // the send node released (pending, not yet executed).
    assert_eq!(run.status, AutomationRunStatus::Queued);
    assert!(run.checkpoint.awaiting_gate.is_none());
    assert_eq!(run.checkpoint.gate_history.len(), 1);
    let record = &run.checkpoint.gate_history[0];
    assert_eq!(record.node_id, "approval_gate");
    assert_eq!(record.decision, "approve");
    assert_eq!(record.reason.as_deref(), Some("LGTM"));
    let decider = record.decided_by.as_ref().expect("decision has an actor");
    assert_eq!(decider.actor_id.as_deref(), Some("reviewer-1"));
    assert!(run
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node| node == "approval_gate"));
    assert_eq!(
        run.checkpoint.node_outputs["approval_gate"]["contract_kind"],
        json!("approval_gate")
    );
    // Run finalization requires a terminal status on the gate output;
    // without it the whole run derives as "terminal accounting missing"
    // (caught by `tandem-engine smoke`, TAN-227).
    assert_eq!(
        run.checkpoint.node_outputs["approval_gate"]["status"],
        json!("completed")
    );
    assert!(
        run.checkpoint.pending_nodes.iter().any(|n| n == "send_email"),
        "send node is released for execution only after approval"
    );
    assert!(!send_email_completed(&run));
}

#[tokio::test]
async fn email_approval_golden_cancel_path_never_sends() {
    let state = ready_test_state().await;
    let automation = email_approval_automation("auto-email-cancel");
    let mut run = paused_email_run(&state, &automation).await;

    let gate = email_pending_gate();
    let outcome = crate::app::state::apply_automation_gate_decision(
        &mut run,
        &automation,
        &gate,
        "cancel",
        Some("wrong recipient".to_string()),
        Some(human_reviewer()),
    );
    assert!(matches!(
        outcome,
        crate::app::state::AutomationGateDecisionOutcome::Applied
    ));

    assert_eq!(run.status, AutomationRunStatus::Cancelled);
    assert_eq!(run.stop_kind, Some(AutomationStopKind::Cancelled));
    assert!(!send_email_completed(&run), "send must never run after cancel");
    assert!(!run
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node| node == "approval_gate"));
    assert_eq!(run.checkpoint.gate_history.len(), 1);
    assert_eq!(run.checkpoint.gate_history[0].decision, "cancel");
    assert!(run
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|entry| entry.event == "run_cancelled"));
}

#[tokio::test]
async fn email_approval_golden_rework_rearms_and_second_approval_releases_send() {
    let state = ready_test_state().await;
    let automation = email_approval_automation("auto-email-rework");
    let mut run = paused_email_run(&state, &automation).await;

    let gate = email_pending_gate();
    let outcome = crate::app::state::apply_automation_gate_decision(
        &mut run,
        &automation,
        &gate,
        "rework",
        Some("tone is off".to_string()),
        Some(human_reviewer()),
    );
    assert!(matches!(
        outcome,
        crate::app::state::AutomationGateDecisionOutcome::Applied
    ));

    // Rework resets compose (and the gate) back to pending and clears the
    // draft output; send is still not executed.
    assert_eq!(run.status, AutomationRunStatus::Queued);
    assert!(run
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node| node == "compose_email"));
    assert!(!run
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node| node == "compose_email"));
    assert!(!run.checkpoint.node_outputs.contains_key("compose_email"));
    assert!(!send_email_completed(&run));
    assert_eq!(run.checkpoint.gate_history.len(), 1);

    // Second round: compose finishes again, gate re-arms, approval releases send.
    run.checkpoint.completed_nodes.push("compose_email".to_string());
    run.checkpoint
        .pending_nodes
        .retain(|node| node != "compose_email");
    crate::app::state::pause_automation_run_for_gate(
        &mut run,
        email_pending_gate(),
        vec!["send_email".to_string()],
    );
    assert_eq!(run.status, AutomationRunStatus::AwaitingApproval);

    let outcome = crate::app::state::apply_automation_gate_decision(
        &mut run,
        &automation,
        &gate,
        "approve",
        None,
        Some(human_reviewer()),
    );
    assert!(matches!(
        outcome,
        crate::app::state::AutomationGateDecisionOutcome::Applied
    ));
    assert_eq!(run.checkpoint.gate_history.len(), 2);
    assert_eq!(run.checkpoint.gate_history[0].decision, "rework");
    assert_eq!(run.checkpoint.gate_history[1].decision, "approve");
    assert_eq!(run.status, AutomationRunStatus::Queued);
    assert!(run.checkpoint.pending_nodes.iter().any(|n| n == "send_email"));
}

#[tokio::test]
async fn email_approval_golden_rejects_decisions_on_settled_gates() {
    let state = ready_test_state().await;
    let automation = email_approval_automation("auto-email-double");
    let mut run = paused_email_run(&state, &automation).await;

    let gate = email_pending_gate();
    let first = crate::app::state::apply_automation_gate_decision(
        &mut run,
        &automation,
        &gate,
        "approve",
        None,
        Some(human_reviewer()),
    );
    assert!(matches!(
        first,
        crate::app::state::AutomationGateDecisionOutcome::Applied
    ));

    // A second decision (e.g. a racing cancel) must not apply: the winner is
    // returned, history stays at one record, and the run state is unchanged.
    let second = crate::app::state::apply_automation_gate_decision(
        &mut run,
        &automation,
        &gate,
        "cancel",
        Some("too late".to_string()),
        Some(human_reviewer()),
    );
    match second {
        crate::app::state::AutomationGateDecisionOutcome::AlreadyDecided(winner) => {
            let winner = winner.expect("winning decision returned");
            assert_eq!(winner.decision, "approve");
        }
        crate::app::state::AutomationGateDecisionOutcome::Applied => {
            panic!("settled gate must not accept a second decision")
        }
    }
    assert_eq!(run.checkpoint.gate_history.len(), 1);
    assert_eq!(run.status, AutomationRunStatus::Queued);
    assert!(!send_email_completed(&run));

    // Decisions for a gate that was never pending are rejected the same way.
    let bogus_gate = AutomationPendingGate {
        node_id: "not_a_gate".to_string(),
        ..email_pending_gate()
    };
    let bogus = crate::app::state::apply_automation_gate_decision(
        &mut run,
        &automation,
        &bogus_gate,
        "approve",
        None,
        Some(human_reviewer()),
    );
    assert!(matches!(
        bogus,
        crate::app::state::AutomationGateDecisionOutcome::AlreadyDecided(_)
    ));
    assert_eq!(run.checkpoint.gate_history.len(), 1);
}
