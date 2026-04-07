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

    assert!(prompt.contains("Code Agent Contract:"));
    assert!(prompt.contains("inspect -> patch -> apply -> test -> repair"));
    assert!(prompt.contains("Do not claim completion until the patch has been applied"));
    assert!(prompt.contains("Run the declared verification command after applying changes"));
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
