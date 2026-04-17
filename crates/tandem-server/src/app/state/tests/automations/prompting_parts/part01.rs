#[test]
fn knowledge_context_is_injected_into_automation_prompt() {
    let automation = AutomationV2Spec {
        automation_id: "automation-knowledge".to_string(),
        name: "Knowledge Prompt".to_string(),
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
        node_id: "strategy-planning".to_string(),
        agent_id: "planner".to_string(),
        objective: "Plan next week's launch strategy".to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    };
    let agent = AutomationAgentProfile {
        agent_id: "planner".to_string(),
        template_id: None,
        display_name: "Planner".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt_with_options(
        &automation,
        "/tmp",
        "run-knowledge",
        &node,
        1,
        &agent,
        &[],
        &["read".to_string()],
        None,
        None,
        None,
        AutomationPromptRenderOptions {
            summary_only_upstream: false,
            knowledge_context: Some(
                "<knowledge_context>\n- reused evidence\n</knowledge_context>".to_string(),
            ),
            runtime_values: None,
        },
    );

    assert!(prompt.contains("<knowledge_context>"));
    assert!(prompt.contains("reused evidence"));
}

#[test]
fn connector_backed_automation_prompt_surfaces_mcp_discovery_guidance() {
    let automation = AutomationV2Spec {
        automation_id: "automation-mcp".to_string(),
        name: "MCP Prompt".to_string(),
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
        node_id: "research_sources".to_string(),
        agent_id: "research".to_string(),
        objective: "Research Reddit threads about AI assistants.".to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "citations".to_string(),
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
        metadata: None,
    };
    let agent = AutomationAgentProfile {
        agent_id: "research".to_string(),
        template_id: None,
        display_name: "Research".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: vec!["github".to_string()],
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-mcp",
        &node,
        1,
        &agent,
        &[],
        &["read".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("MCP Discovery:"));
    assert!(prompt.contains("Call `mcp_list`"));
    assert!(prompt.contains("Allowed MCP servers"));
}

#[test]
fn compare_results_prompt_prioritizes_mcp_discovery_and_artifact_delivery() {
    let automation = AutomationV2Spec {
        automation_id: "automation-compare-results".to_string(),
        name: "Compare Results".to_string(),
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
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
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
        node_id: "compare_results".to_string(),
        agent_id: "analyst".to_string(),
        objective: "Compare the gathered evidence and write the final comparison.".to_string(),
        depends_on: vec!["collect_inputs".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "collect_inputs".to_string(),
            alias: "comparison_inputs".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Write the comparison as the run artifact.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "title": "Compare Results",
                "role": "analyst",
                "prompt": "Compare the gathered evidence, call mcp_list when connector-backed sources may matter, and write the final comparison.",
                "output_path": ".tandem/artifacts/compare-results.md"
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "analyst".to_string(),
        template_id: None,
        display_name: "Analyst".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec![
                "glob".to_string(),
                "read".to_string(),
                "write".to_string(),
                "mcp_list".to_string(),
            ],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: vec!["blog-mcp".to_string()],
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-compare",
        &node,
        1,
        &agent,
        &[],
        &[
            "glob".to_string(),
            "read".to_string(),
            "write".to_string(),
            "mcp_list".to_string(),
        ],
        None,
        None,
        None,
    );

    assert!(prompt.contains("MCP Discovery:"));
    assert!(prompt.contains("Artifact Delivery Order:"));
    assert!(prompt.contains("Call `mcp_list` before reading or comparing sources"));
    assert!(prompt.contains(
        "Write the required run artifact to `.tandem/runs/run-compare/artifacts/compare-results.md`"
    ));
    assert!(prompt.contains("Artifact Delivery Fallback:"));
    assert!(prompt.contains("finish the artifact from the local evidence you already have"));
    assert!(prompt.contains(
        "On retries, rewrite the file in the current attempt even if the content is identical."
    ));
}

#[test]
fn prompt_separates_read_only_source_of_truth_files_from_write_targets() {
    let automation = AutomationV2Spec {
        automation_id: "automation-read-only-source-truth".to_string(),
        name: "Read Only Source Truth".to_string(),
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
        node_id: "assess".to_string(),
        agent_id: "research".to_string(),
        objective: "Read RESUME.md as the source of truth for skills, role targets, and geography preferences. If resume_overview.md does not exist, create it. Create or append daily_results_2026-04-15.md in the workspace root and keep RESUME.md untouched.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return triage JSON.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "title": "Assess",
                "role": "watcher",
                "prompt": "Return a JSON triage handoff."
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "research".to_string(),
        template_id: None,
        display_name: "Research".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string(), "write".to_string()],
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
        "run-read-only-source-truth",
        &node,
        1,
        &agent,
        &[],
        &["read".to_string(), "write".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Read-Only Source Files:"));
    assert!(prompt.contains("RESUME.md"));
    assert!(prompt.contains("Required Workspace Writes:"));
    assert!(prompt.contains("resume_overview.md"));
    assert!(prompt.contains("daily_results_2026-04-15.md"));
    assert!(prompt.contains("Treat these named files as input-only source-of-truth files"));
    assert!(prompt.contains("structured_handoff.source_material"));
}

#[test]
fn automation_prompt_clarifies_file_paths_are_not_directories() {
    let automation = AutomationV2Spec {
        automation_id: "automation-file-paths".to_string(),
        name: "File Path Prompt".to_string(),
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
        node_id: "execute_goal".to_string(),
        agent_id: "worker".to_string(),
        objective: "Bootstrap files in the workspace and update tracker/seen-jobs.jsonl."
            .to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
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
        stage_kind: None,
        gate: None,
        metadata: None,
    };
    let agent = AutomationAgentProfile {
        agent_id: "worker".to_string(),
        template_id: None,
        display_name: "Worker".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["write".to_string()],
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
        "run-file-paths",
        &node,
        1,
        &agent,
        &[],
        &["write".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Create only parent folders as directories"));
    assert!(prompt.contains("Do not use `bash`/`mkdir` to create a file path itself"));
}

#[test]
fn bootstrap_prompt_allows_required_workspace_writes_beyond_run_artifact() {
    let automation = AutomationV2Spec {
        automation_id: "automation-bootstrap".to_string(),
        name: "Bootstrap Prompt".to_string(),
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
        node_id: "collect_inputs".to_string(),
        agent_id: "worker".to_string(),
        objective: "Initialize any missing job-search workspace directories and files, read README.md/AGENTS.md/RESUME.md if present, and update resume-overview.md, resume-positioning.md, resume-state.json, sources/search-targets.md, tracker/search-ledger/2026-04-09.json, tracker/seen-jobs.jsonl, tracker/pipeline.md, and daily-recaps/2026-04-09-job-search-recap.md as needed before any search begins.".to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
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
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/collect-inputs.json"
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "worker".to_string(),
        template_id: None,
        display_name: "Worker".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["glob".to_string(), "read".to_string(), "write".to_string()],
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
        "run-bootstrap",
        &node,
        1,
        &agent,
        &[],
        &["glob".to_string(), "read".to_string(), "write".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Required Workspace Writes:"));
    assert!(prompt.contains("resume-overview.md"));
    assert!(prompt.contains("tracker/pipeline.md"));
    assert!(prompt.contains("daily-recaps/2026-04-09-job-search-recap.md"));
    assert!(prompt.contains(
        "Use only approved write targets for this node: the declared run artifact plus these required workspace files"
    ));
    assert!(!prompt.contains("Use only declared workflow artifact paths."));
    assert!(!prompt.contains("Only write declared workflow artifact files."));
    assert!(!prompt.contains("External Research Expectation:"));
    assert!(!prompt.contains("Call `websearch` now"));
}

#[test]
fn bootstrap_prompt_keeps_source_of_truth_reads_visible_with_optional_workspace_writes() {
    let automation = AutomationV2Spec {
        automation_id: "automation-bootstrap-source".to_string(),
        name: "Bootstrap Prompt Source".to_string(),
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
        node_id: "collect_inputs".to_string(),
        agent_id: "worker".to_string(),
        objective: "Read RESUME.md as the source of truth for skills, role targets, seniority, technologies, and geography preferences. If resume_overview.md does not exist, create it. Create or append daily_results_2026-04-15.md in the workspace root and keep RESUME.md untouched.".to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
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
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/collect-inputs.json",
                "prompt": "Return a JSON triage handoff."
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "worker".to_string(),
        template_id: None,
        display_name: "Worker".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["glob".to_string(), "read".to_string(), "write".to_string()],
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
        "run-bootstrap-source",
        &node,
        1,
        &agent,
        &[],
        &["glob".to_string(), "read".to_string(), "write".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Concrete Source Coverage:"));
    assert!(prompt.contains("RESUME.md"));
    assert!(prompt.contains("Similar backup or copy filenames do not satisfy the requirement"));
    assert!(prompt.contains("Read-Only Source Files:"));
    assert!(prompt.contains("Required Workspace Writes:"));
    assert!(prompt.contains("resume_overview.md"));
    assert!(prompt.contains("daily_results_2026-04-15.md"));
}

#[test]
fn generated_prompt_variation_suite_preserves_contract_inference() {
    struct GeneratedPromptCase {
        name: &'static str,
        node: AutomationFlowNode,
        requested_tools: Vec<String>,
        allowed_servers: Vec<String>,
        expected_present: Vec<&'static str>,
        expected_absent: Vec<&'static str>,
    }

    let automation = AutomationV2Spec {
        automation_id: "automation-generated-prompt-variations".to_string(),
        name: "Generated Prompt Variations".to_string(),
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

    let cases = vec![
        GeneratedPromptCase {
            name: "filesystem-bootstrap",
            node: AutomationFlowNode {
                node_id: "collect_inputs".to_string(),
                agent_id: "worker".to_string(),
                objective: "Filesystem-only initialization: resolve current_date/current_time, ensure run folders exist, and write run-context.md."
                    .to_string(),
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
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
                stage_kind: None,
                gate: None,
                metadata: Some(json!({
                    "builder": {
                        "output_path": ".tandem/artifacts/collect-inputs.json",
                        "output_files": ["run-context.md"]
                    }
                })),
            },
            requested_tools: vec!["glob".to_string(), "read".to_string(), "write".to_string()],
            allowed_servers: Vec::new(),
            expected_present: vec![
                "Required Workspace Writes:",
                "run-context.md",
                "Execution Policy:",
            ],
            expected_absent: vec![
                "External Research Expectation:",
                "Call `websearch` now",
                "MCP Discovery:",
            ],
        },
        GeneratedPromptCase {
            name: "web-grounded-brief",
            node: AutomationFlowNode {
                node_id: "research_web".to_string(),
                agent_id: "researcher".to_string(),
                objective: "Research the current external workflow testing landscape and write a grounded brief."
                    .to_string(),
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(AutomationFlowOutputContract {
                    kind: "brief".to_string(),
                    validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
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
                        "output_path": ".tandem/artifacts/web-brief.md",
                        "web_research_expected": true,
                        "source_coverage_required": true
                    }
                })),
            },
            requested_tools: vec![
                "glob".to_string(),
                "read".to_string(),
                "websearch".to_string(),
                "write".to_string(),
            ],
            allowed_servers: Vec::new(),
            expected_present: vec![
                "External Research Expectation:",
                "Call `websearch` now",
                "Artifact Delivery Order:",
            ],
            expected_absent: vec!["Required Workspace Writes:"],
        },
        GeneratedPromptCase {
            name: "mcp-grounded-citations",
            node: AutomationFlowNode {
                node_id: "research_sources".to_string(),
                agent_id: "researcher".to_string(),
                objective: "Ground the run in Tandem docs using tandem-mcp before producing a citations handoff."
                    .to_string(),
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(AutomationFlowOutputContract {
                    kind: "citations".to_string(),
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
                        "output_path": ".tandem/artifacts/research-sources.json",
                        "preferred_mcp_servers": ["tandem-mcp"]
                    }
                })),
            },
            requested_tools: vec![
                "mcp_list".to_string(),
                "mcp.tandem_mcp.search_docs".to_string(),
                "write".to_string(),
            ],
            allowed_servers: vec!["tandem-mcp".to_string()],
            expected_present: vec![
                "MCP Discovery:",
                "Allowed MCP servers",
                "Call `mcp_list`",
                "Artifact Delivery Order:",
            ],
            expected_absent: vec!["External Research Expectation:"],
        },
        GeneratedPromptCase {
            name: "code-change-with-verification",
            node: AutomationFlowNode {
                node_id: "implement_fix".to_string(),
                agent_id: "engineer".to_string(),
                objective: "Inspect the code, patch the smallest root cause, rerun verification, and write a handoff."
                    .to_string(),
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(AutomationFlowOutputContract {
                    kind: "report_markdown".to_string(),
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
                        "task_kind": "code_change",
                        "verification_command": "cargo test",
                        "output_path": ".tandem/artifacts/code-loop.md"
                    }
                })),
            },
            requested_tools: vec![
                "read".to_string(),
                "edit".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ],
            allowed_servers: Vec::new(),
            expected_present: vec![
                "Prefer `edit` for existing-file changes.",
                "cargo test",
                "Required Run Artifact:",
            ],
            expected_absent: vec!["External Research Expectation:"],
        },
    ];

    for case in cases {
        let agent = AutomationAgentProfile {
            agent_id: case.node.agent_id.clone(),
            template_id: None,
            display_name: "Generated Agent".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: case.requested_tools.clone(),
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: case.allowed_servers.clone(),
                allowed_tools: None,
            },
            approval_policy: None,
        };

        let prompt = render_automation_v2_prompt(
            &automation,
            "/tmp",
            &format!("run-{}", case.name),
            &case.node,
            1,
            &agent,
            &[],
            &case.requested_tools,
            None,
            None,
            None,
        );

        for expected in case.expected_present {
            assert!(
                prompt.contains(expected),
                "case={} missing expected prompt fragment {:?}\n{}",
                case.name,
                expected,
                prompt
            );
        }
        for forbidden in case.expected_absent {
            assert!(
                !prompt.contains(forbidden),
                "case={} unexpectedly contained prompt fragment {:?}\n{}",
                case.name,
                forbidden,
                prompt
            );
        }
    }
}

#[test]
fn later_nodes_inherit_automation_wide_read_only_source_truth_guidance() {
    let automation = AutomationV2Spec {
        automation_id: "automation-global-read-only".to_string(),
        name: "Global Read Only".to_string(),
        description: Some(
            "Analyze RESUME.md and use it as the source of truth. Never edit, rewrite, rename, move, or delete RESUME.md."
                .to_string(),
        ),
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
        flow: crate::AutomationFlowSpec {
            nodes: vec![AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "assess".to_string(),
                agent_id: "worker".to_string(),
                objective: "Read RESUME.md as the source of truth and confirm whether resume_overview.md already exists.".to_string(),
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
        workspace_root: Some("/home/evan/job-hunt".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "worker".to_string(),
        objective:
            "Create or append daily_results_2026-04-15.md in the workspace root and return a short append-safe report."
                .to_string(),
        depends_on: vec!["assess".to_string()],
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
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_files": ["daily_results_2026-04-15.md"]
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "worker".to_string(),
        template_id: None,
        display_name: "Worker".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string(), "write".to_string()],
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
        "/home/evan/job-hunt",
        "run-global-read-only",
        &node,
        1,
        &agent,
        &[],
        &["read".to_string(), "write".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Read-Only Source Files:"));
    assert!(prompt.contains("RESUME.md"));
    assert!(prompt.contains("daily_results_2026-04-15.md"));
    assert!(!prompt.contains("Use only approved write targets for this node: the declared run artifact plus these required workspace files: `RESUME.md`"));
}

#[test]
fn prompt_resolves_reserved_runtime_placeholders_for_run() {
    let automation = AutomationV2Spec {
        automation_id: "automation-runtime-placeholders".to_string(),
        name: "Runtime Placeholder Prompt".to_string(),
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
        output_targets: vec!["daily-recaps/{current_date}-job-search-recap.md".to_string()],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: Some(json!({
            "mission": {
                "goal": "Write daily-recaps/{current_date}-job-search-recap.md and opportunities/raw/{current_date}/{current_time}-findings.md."
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        node_id: "execute_goal".to_string(),
        agent_id: "worker".to_string(),
        objective: "Write tracker/search-ledger/{current_date}.json and use {current_timestamp} inside the recap.".to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        depends_on: vec!["collect_inputs".to_string()],
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
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "prompt": "Also preserve opportunities/raw/{current_date}/{current_time}-findings.json."
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "worker".to_string(),
        template_id: None,
        display_name: "Worker".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["glob".to_string(), "read".to_string(), "write".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt_with_options(
        &automation,
        "/tmp",
        "run-runtime-placeholders",
        &node,
        1,
        &agent,
        &[],
        &["glob".to_string(), "read".to_string(), "write".to_string()],
        None,
        None,
        None,
        AutomationPromptRenderOptions {
            summary_only_upstream: false,
            knowledge_context: None,
            runtime_values: Some(runtime_values("2026-04-09", "0935", "2026-04-09 09:35")),
        },
    );

    assert!(prompt.contains("Resolved Runtime Values:"));
    assert!(prompt.contains("`current_date` = `2026-04-09`"));
    assert!(prompt.contains("tracker/search-ledger/2026-04-09.json"));
    assert!(prompt.contains("opportunities/raw/2026-04-09/0935-findings.json"));
    assert!(prompt.contains("2026-04-09 09:35"));
    assert!(!prompt.contains("tracker/search-ledger/{current_date}.json"));
    assert!(!prompt.contains("opportunities/raw/{current_date}/{current_time}-findings.json"));
    assert!(!prompt.contains("use {current_timestamp} inside the recap"));
}

#[test]
fn final_prompt_surfaces_automation_output_targets_as_required_workspace_writes() {
    let automation = AutomationV2Spec {
        automation_id: "automation-final-targets".to_string(),
        name: "Final Targets".to_string(),
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
        output_targets: vec![
            "daily-recaps/{current_date}-job-search-recap.md".to_string(),
            "opportunities/ranked/{current_date}-ranked-opportunities.md".to_string(),
            "opportunities/shortlisted/{current_date}-shortlist.md".to_string(),
            "tracker/pipeline.md".to_string(),
            "opportunities/raw/{current_date}/{current_time}-findings.md".to_string(),
        ],
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
        node_id: "analyze_findings".to_string(),
        agent_id: "worker".to_string(),
        objective: "Normalize worthwhile jobs, update daily ranked opportunities, shortlist, and pipeline views, then merge the daily recap.".to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        depends_on: vec!["research_sources".to_string()],
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
        metadata: None,
    };
    let agent = AutomationAgentProfile {
        agent_id: "worker".to_string(),
        template_id: None,
        display_name: "Worker".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["glob".to_string(), "read".to_string(), "write".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt_with_options(
        &automation,
        "/tmp",
        "run-final-targets",
        &node,
        1,
        &agent,
        &[],
        &["glob".to_string(), "read".to_string(), "write".to_string()],
        None,
        None,
        None,
        AutomationPromptRenderOptions {
            summary_only_upstream: false,
            knowledge_context: None,
            runtime_values: Some(runtime_values("2026-04-09", "1304", "2026-04-09 13:04")),
        },
    );

    assert!(
        !prompt.contains("Required Workspace Writes:"),
        "Required Workspace Writes should not appear for a downstream node with no output_files"
    );
}

#[test]
fn structured_json_prompt_surfaces_explicit_output_files_for_analyze_findings() {
    let automation = AutomationV2Spec {
        automation_id: "automation-analyze-findings".to_string(),
        name: "Analyze Findings".to_string(),
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
        node_id: "analyze_findings".to_string(),
        agent_id: "analyst".to_string(),
        objective: "Synthesize the clustered findings into a structured analysis and update the durable summary file.".to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        depends_on: vec!["cluster_topics".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "cluster_topics".to_string(),
            alias: "clusters".to_string(),
        }],
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
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/analyze-findings.json",
                "output_files": ["reports/pain-points-analysis.md"]
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "analyst".to_string(),
        template_id: None,
        display_name: "Analyst".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["glob".to_string(), "read".to_string(), "write".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt_with_options(
        &automation,
        "/tmp",
        "run-analyze-findings",
        &node,
        1,
        &agent,
        &[],
        &["glob".to_string(), "read".to_string(), "write".to_string()],
        None,
        None,
        None,
        AutomationPromptRenderOptions {
            summary_only_upstream: false,
            knowledge_context: None,
            runtime_values: None,
        },
    );

    assert!(prompt.contains("Required Workspace Writes:"));
    assert!(prompt.contains("reports/pain-points-analysis.md"));
    assert!(prompt.contains(
        "Write the required run artifact to `.tandem/runs/run-analyze-findings/artifacts/analyze-findings.json`"
    ));
    assert!(!prompt.contains(".tandem/artifacts/analyze-findings.json"));
    assert!(prompt.contains(
        "Use only approved write targets for this node: the declared run artifact plus these required workspace files"
    ));
    assert!(prompt.contains(
        "In addition to the run artifact, create or update these required workspace files when needed: `reports/pain-points-analysis.md`."
    ));
}

