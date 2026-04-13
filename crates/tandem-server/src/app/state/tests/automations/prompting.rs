use super::*;

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
            runtime_values: Some(AutomationPromptRuntimeValues {
                current_date: "2026-04-09".to_string(),
                current_time: "0935".to_string(),
                current_timestamp: "2026-04-09 09:35".to_string(),
            }),
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
            runtime_values: Some(AutomationPromptRuntimeValues {
                current_date: "2026-04-09".to_string(),
                current_time: "1304".to_string(),
                current_timestamp: "2026-04-09 13:04".to_string(),
            }),
        },
    );

    assert!(
        !prompt.contains("Required Workspace Writes:"),
        "Required Workspace Writes should not appear for a downstream node with no output_files"
    );
}

#[test]
fn first_attempt_research_prompt_requires_completed_status() {
    let automation = AutomationV2Spec {
        automation_id: "automation-1".to_string(),
        name: "Research Automation".to_string(),
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
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing-brief.md".to_string(),
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
                "output_path": "marketing-brief.md",
                "source_coverage_required": true,
                "web_research_expected": true
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
            allowlist: vec![
                "glob".to_string(),
                "read".to_string(),
                "websearch".to_string(),
                "write".to_string(),
            ],
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
        "run-1",
        &node,
        1,
        &agent,
        &[],
        &[
            "glob".to_string(),
            "read".to_string(),
            "websearch".to_string(),
            "write".to_string(),
        ],
        None,
        None,
        None,
    );

    assert!(prompt.contains("`status` set to `completed`"));
    assert!(prompt.contains("Do not declare the output blocked"));
    assert!(!prompt.contains("at least `status` (`completed` or `blocked`)"));
}

#[test]
fn code_patch_prompt_includes_code_agent_contract_instructions() {
    let automation = AutomationV2Spec {
        automation_id: "automation-code-patch".to_string(),
        name: "Code Patch Prompt".to_string(),
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
        node_id: "code_patch".to_string(),
        agent_id: "coder".to_string(),
        objective: "Patch the code and verify the change.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "code_patch".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::CodePatch),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Produce a patch-backed artifact.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "task_kind": "code_change",
                "output_path": "src/lib.rs",
                "verification_command": "cargo test"
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "coder".to_string(),
        template_id: None,
        display_name: "Coder".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec![
                "read".to_string(),
                "edit".to_string(),
                "apply_patch".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ],
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
        "run-code",
        &node,
        1,
        &agent,
        &[],
        &[
            "read".to_string(),
            "edit".to_string(),
            "apply_patch".to_string(),
            "write".to_string(),
            "bash".to_string(),
        ],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Coding Task Context:"));
    assert!(prompt.contains("Verification expectation:"));
    assert!(prompt.contains("Use `bash` for verification commands when tool access allows it."));
    assert!(prompt.contains("Required Run Artifact:"));
}

#[test]
fn automation_node_required_tools_reads_builder_metadata() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "artifact".to_string(),
        agent_id: "writer".to_string(),
        objective: "Write notes.md".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "artifact".to_string(),
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
                "output_path": "notes.md",
                "required_tools": ["read", "websearch"]
            }
        })),
    };

    assert_eq!(
        automation_node_required_tools(&node),
        vec!["read".to_string(), "websearch".to_string()]
    );
}

#[test]
fn filter_requested_tools_to_available_removes_unconfigured_websearch() {
    let requested_tools = vec![
        "glob".to_string(),
        "read".to_string(),
        "websearch".to_string(),
        "write".to_string(),
    ];
    let available_tool_names = ["glob", "read", "write"]
        .into_iter()
        .map(str::to_string)
        .collect::<HashSet<_>>();

    let filtered = filter_requested_tools_to_available(requested_tools, &available_tool_names);

    assert_eq!(
        filtered,
        vec!["glob".to_string(), "read".to_string(), "write".to_string()]
    );
}

#[test]
fn wildcard_automation_allowlist_expands_to_minimal_research_tools() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Research current web sources and collect citations.".to_string(),
        depends_on: Vec::new(),
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "collect_inputs".to_string(),
            alias: "research_brief".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "citations".to_string(),
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
                "output_path": ".tandem/artifacts/research-sources.json"
            }
        })),
    };

    let requested = normalize_automation_requested_tools(&node, "/tmp", vec!["*".to_string()]);

    assert_eq!(
        requested,
        vec![
            "glob".to_string(),
            "read".to_string(),
            "websearch".to_string(),
            "write".to_string()
        ]
    );
}

#[test]
fn wildcard_automation_allowlist_keeps_email_delivery_tools_narrow() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "notify_user".to_string(),
        agent_id: "committer".to_string(),
        objective: "Send the finalized report to the requested email address.".to_string(),
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
        metadata: Some(json!({
            "delivery": {
                "method": "email",
                "to": "recipient@example.com",
                "content_type": "text/html",
                "inline_body_only": true,
                "attachments": false
            }
        })),
    };
    let available_tool_names = [
        "glob".to_string(),
        "read".to_string(),
        "mcp.gmail_alt.gmail_send_draft".to_string(),
        "mcp.gmail_alt.gmail_send_email".to_string(),
        "mcp.github.issues_list".to_string(),
    ]
    .into_iter()
    .collect::<HashSet<_>>();
    let requested = automation_requested_tools_for_node(
        &node,
        "/tmp",
        vec!["*".to_string()],
        &available_tool_names,
    );

    assert!(!requested.iter().any(|tool| tool == "*"));
    assert!(requested
        .iter()
        .any(|tool| tool == "mcp.gmail_alt.gmail_send_email"));
    assert!(requested
        .iter()
        .any(|tool| tool == "mcp.gmail_alt.gmail_send_draft"));
    assert!(!requested
        .iter()
        .any(|tool| tool == "mcp.github.issues_list"));
}

#[test]
fn wildcard_automation_allowlist_recognizes_outlook_reply_and_compose_email_tools() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "notify_user".to_string(),
        agent_id: "committer".to_string(),
        objective: "Send the finalized report to the requested email address.".to_string(),
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
        metadata: Some(json!({
            "delivery": {
                "method": "email",
                "to": "recipient@example.com",
                "content_type": "text/html",
                "inline_body_only": true,
                "attachments": false
            }
        })),
    };
    let available_tool_names = [
        "glob".to_string(),
        "read".to_string(),
        "mcp.outlook.reply_message".to_string(),
        "mcp.outlook.compose_draft".to_string(),
        "mcp.github.issues_list".to_string(),
    ]
    .into_iter()
    .collect::<HashSet<_>>();
    let requested = automation_requested_tools_for_node(
        &node,
        "/tmp",
        vec!["*".to_string()],
        &available_tool_names,
    );

    assert!(requested
        .iter()
        .any(|tool| tool == "mcp.outlook.reply_message"));
    assert!(requested
        .iter()
        .any(|tool| tool == "mcp.outlook.compose_draft"));
    assert!(!requested
        .iter()
        .any(|tool| tool == "mcp.github.issues_list"));
}

#[test]
fn structured_json_prompt_requires_json_only_without_follow_up_questions() {
    let automation = AutomationV2Spec {
        automation_id: "automation-json-only".to_string(),
        name: "JSON Only".to_string(),
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
        node_id: "research-external-research".to_string(),
        agent_id: "research".to_string(),
        objective: "Perform external research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return structured JSON.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "title": "External Research",
                "role": "watcher",
                "prompt": "Return structured research handoff.",
                "research_stage": "research_external_sources"
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
            allowlist: vec![
                "glob".to_string(),
                "read".to_string(),
                "webfetch".to_string(),
            ],
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
        "run-json-only",
        &node,
        2,
        &agent,
        &[],
        &[
            "glob".to_string(),
            "read".to_string(),
            "webfetch".to_string(),
        ],
        None,
        None,
        None,
    );

    assert!(prompt.contains("The final response body should contain JSON only"));
    assert!(prompt.contains("Do not include headings, bullets, markdown fences, prose explanations, or follow-up questions"));
}

#[test]
fn handoff_only_structured_json_prompt_strips_internal_context_writes() {
    let automation = AutomationV2Spec {
        automation_id: "automation-json-context".to_string(),
        name: "JSON Context".to_string(),
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
        node_id: "assess".to_string(),
        agent_id: "research".to_string(),
        objective: "Assess the workspace state".to_string(),
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
            allowlist: vec!["glob".to_string(), "read".to_string(), "write".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };
    let upstream_inputs = vec![json!({
        "alias": "runtime_context_partition",
        "from_step_id": "planner",
        "output": {
            "content": {
                "text": "{\"step_context_bindings\":{\"assess\":{\"context_writes\":[\"ctx:wfplan-123:assess:assess.artifact\"],\"summary\":\"triage context\"}}}"
            }
        }
    })];

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-json-context",
        &node,
        1,
        &agent,
        &upstream_inputs,
        &["glob".to_string(), "read".to_string()],
        None,
        None,
        None,
    );

    assert!(!prompt.contains("context_writes"));
    assert!(!prompt.contains("ctx:wfplan-123:assess:assess.artifact"));
    assert!(prompt.contains("internal context identifiers"));
    assert!(prompt.contains(
        "Do not call `write` unless this node explicitly declares a workflow output path."
    ));
}

#[test]
fn json_output_artifact_prompt_requires_response_body_backup_copy() {
    let automation = AutomationV2Spec {
        automation_id: "automation-json-artifact".to_string(),
        name: "JSON Artifact".to_string(),
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
        node_id: "research_sources".to_string(),
        agent_id: "research".to_string(),
        objective: "Inspect the workspace and record the relevant sources.".to_string(),
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
                "output_path": ".tandem/artifacts/research-sources.json"
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
        "run-json-artifact",
        &node,
        2,
        &agent,
        &[],
        &["glob".to_string(), "read".to_string(), "write".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("If the required run artifact is JSON, also include the exact JSON artifact body in the final response before the compact status object"));
}

#[test]
fn external_research_prompt_handles_missing_websearch_tool() {
    let automation = AutomationV2Spec {
        automation_id: "automation-external-fallback".to_string(),
        name: "External Fallback".to_string(),
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
        node_id: "research-external-research".to_string(),
        agent_id: "research".to_string(),
        objective: "Perform targeted external research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return structured JSON.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "title": "External Research",
                "role": "watcher",
                "prompt": "Use external tools and return structured findings.",
                "research_stage": "research_external_sources",
                "web_research_expected": true
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
            allowlist: vec![
                "glob".to_string(),
                "read".to_string(),
                "webfetch".to_string(),
            ],
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
        "run-external-fallback",
        &node,
        2,
        &agent,
        &[],
        &[
            "glob".to_string(),
            "read".to_string(),
            "webfetch".to_string(),
        ],
        None,
        None,
        None,
    );

    assert!(prompt.contains("`websearch` is not available in this run"));
    assert!(prompt.contains("Use `webfetch` only for concrete URLs already present in local sources or upstream handoffs"));
    assert!(prompt.contains("Do not ask the user for clarification or permission to continue"));
}

#[test]
fn render_prompt_normalizes_upstream_research_paths_from_sources_root() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-upstream-path-normalization-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("tandem-reference/readmes"))
        .expect("create readmes dir");
    std::fs::write(
        workspace_root.join("tandem-reference/SOURCES.md"),
        "# Sources",
    )
    .expect("seed sources");
    std::fs::write(
        workspace_root.join("tandem-reference/readmes/repo-README.md"),
        "# Repo",
    )
    .expect("seed repo readme");

    let automation = AutomationV2Spec {
        automation_id: "automation-upstream-paths".to_string(),
        name: "Upstream Paths".to_string(),
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
        workspace_root: Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-local-sources".to_string(),
        agent_id: "research".to_string(),
        objective: "Read prioritized local files".to_string(),
        depends_on: vec!["research-discover-sources".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "research-discover-sources".to_string(),
            alias: "source_inventory".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return structured JSON.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "title": "Read Local Sources",
                "role": "watcher",
                "prompt": "Use the upstream handoff as the file plan.",
                "research_stage": "research_local_sources"
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
            allowlist: vec!["read".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };
    let upstream_inputs = vec![json!({
        "alias": "source_inventory",
        "from_step_id": "research-discover-sources",
        "output": {
            "artifact_validation": {
                "read_paths": ["tandem-reference/SOURCES.md"],
                "current_node_read_paths": ["tandem-reference/SOURCES.md"]
            },
            "content": {
                "structured_handoff": {
                    "discovered_paths": ["readmes/repo-README.md"],
                    "priority_paths": ["readmes/repo-README.md"]
                },
                "text": "{\"discovered_paths\":[\"readmes/repo-README.md\"],\"priority_paths\":[\"readmes/repo-README.md\"]}"
            }
        }
    })];

    let prompt = render_automation_v2_prompt(
        &automation,
        workspace_root.to_str().expect("workspace root string"),
        "run-upstream-paths",
        &node,
        1,
        &agent,
        &upstream_inputs,
        &["read".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("tandem-reference/readmes/repo-README.md"));

    let _ = std::fs::remove_dir_all(workspace_root);
}
