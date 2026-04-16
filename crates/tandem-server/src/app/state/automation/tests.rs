use super::node_runtime_impl::automation_node_should_surface_mcp_discovery;
use super::*;
use crate::automation_v2::types::{AutomationFlowInputRef, AutomationFlowNode};
use serde_json::json;
use std::collections::BTreeMap;
use tandem_types::{ToolCapabilities, ToolDomain, ToolEffect, ToolSchema};

// ---------------------------------------------------------------------------
// Phase-0 smoke tests — regression safety net for module extraction.
// Covers the 4 highest-traffic pure functions identified in
// AUTOMATION_MODULARIZATION_PLAN.md §Pre-Extraction Test Safety Net.
//
// These tests verify observable behaviour before any code moves happen, so
// that a broken import or wrong re-export is caught immediately by `cargo test`.
// ---------------------------------------------------------------------------

// -----------------------------------------------------------------------
// Helpers
// -----------------------------------------------------------------------

fn bare_node() -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "n1".to_string(),
        agent_id: "a1".to_string(),
        objective: "do something".to_string(),
        depends_on: vec![],
        input_refs: vec![],
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    }
}

fn runtime_values(date: &str, time: &str, timestamp: &str) -> AutomationPromptRuntimeValues {
    let time_hms = if time.len() == 4 {
        format!("{time}00")
    } else {
        time.to_string()
    };
    let timestamp_filename = if time.len() == 4 {
        format!("{}_{}-{}-00", date, &time[..2], &time[2..])
    } else {
        format!("{}_{}", date, time)
    };
    AutomationPromptRuntimeValues {
        current_date: date.to_string(),
        current_time: time.to_string(),
        current_timestamp: timestamp.to_string(),
        current_date_compact: date.replace('-', ""),
        current_time_hms: time_hms,
        current_timestamp_filename: timestamp_filename,
    }
}

fn node_with_input_ref() -> AutomationFlowNode {
    let mut node = bare_node();
    node.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "prev".to_string(),
        alias: "research".to_string(),
    }];
    node
}

fn code_workflow_node() -> AutomationFlowNode {
    // automation_node_is_code_workflow checks metadata.builder.task_kind first.
    let mut node = bare_node();
    node.metadata = Some(json!({
        "builder": { "task_kind": "code_change" }
    }));
    node
}

fn task_class_only_node() -> AutomationFlowNode {
    let mut node = bare_node();
    node.metadata = Some(json!({
        "builder": { "task_class": "code_change" }
    }));
    node
}

fn repo_fix_workflow_node() -> AutomationFlowNode {
    let mut node = bare_node();
    node.node_id = "repo_fix_task".to_string();
    node.metadata = Some(json!({
        "builder": { "task_kind": "repo_fix" }
    }));
    node
}

fn research_brief_node() -> AutomationFlowNode {
    let mut node = bare_node();
    node.node_id = "research_brief_task".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "research_brief".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node
}

fn code_patch_contract_node() -> AutomationFlowNode {
    let mut node = bare_node();
    node.node_id = "code_patch".to_string();
    node.objective = "Patch the code and verify the change.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "code_patch".to_string(),
        validator: None,
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": "src/lib.rs",
            "verification_command": "cargo test"
        }
    }));
    node
}

fn email_delivery_node() -> AutomationFlowNode {
    let mut node = bare_node();
    node.objective = "Send the finalized report to the requested email address.".to_string();
    node.metadata = Some(json!({
        "delivery": {
            "method": "email",
            "to": "recipient@example.com",
            "content_type": "text/html",
            "inline_body_only": true,
            "attachments": false
        }
    }));
    node
}

fn generic_research_artifact_node() -> AutomationFlowNode {
    let mut node = bare_node();
    node.node_id = "summarize_resume_signals".to_string();
    node.objective = "Summarize the resume signals into a structured working summary.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": { "task_class": "synthesis" }
    }));
    node
}

fn report_markdown_node() -> AutomationFlowNode {
    let mut node = node_with_input_ref();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node
}

fn automation_with_output_targets(
    nodes: Vec<AutomationFlowNode>,
    output_targets: Vec<String>,
) -> AutomationV2Spec {
    AutomationV2Spec {
        automation_id: "automation-live-output".to_string(),
        name: "Live Output".to_string(),
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
        flow: crate::AutomationFlowSpec { nodes },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets,
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp/workspace".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

fn automation_with_live_output_target(nodes: Vec<AutomationFlowNode>) -> AutomationV2Spec {
    automation_with_output_targets(
        nodes,
        vec![
            "sales/genz-sponsor-research/{current_date}_{current_time}_genz_sponsor_targets.md"
                .to_string(),
        ],
    )
}

fn local_citations_contract_node() -> AutomationFlowNode {
    let mut node = bare_node();
    node.node_id = "research_sources".to_string();
    node.objective =
        "Inspect the local workspace and cite the most relevant project-authored sources."
            .to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "citations".to_string(),
        validator: None,
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "web_research_expected": false
        }
    }));
    node
}

fn mcp_citations_contract_node() -> AutomationFlowNode {
    let mut node = bare_node();
    node.node_id = "research_sources".to_string();
    node.objective =
        "Use tandem-mcp first to study Tandem's supported product truths and save grounded notes."
            .to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "citations".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "preferred_mcp_servers": ["tandem-mcp"],
            "web_research_expected": false
        }
    }));
    node
}

#[test]
fn repair_automation_output_contracts_recovers_report_nodes_and_input_refs() {
    let mut draft = bare_node();
    draft.node_id = "draft_deliverable".to_string();
    draft.objective = "Write the final report to reports/agent_automation_painpoints_YYYY-MM-DD_HH-MM-SS.md using the upstream evidence.".to_string();
    draft.depends_on = vec!["refine_results".to_string()];
    draft.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: Some(json!({"type": "object"})),
        summary_guidance: None,
    });

    let mut finalize = bare_node();
    finalize.node_id = "finalize_outputs".to_string();
    finalize.objective = "Finalize and save reports/agent_automation_painpoints_YYYY-MM-DD_HH-MM-SS.md after rereading the strongest upstream artifacts.".to_string();
    finalize.depends_on = vec!["draft_deliverable".to_string()];
    finalize.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: Some(json!({"type": "object"})),
        summary_guidance: None,
    });

    let mut automation = automation_with_output_targets(
        vec![bare_node(), draft, finalize],
        vec!["reports/agent_automation_painpoints_YYYY-MM-DD_HH-MM-SS.md".to_string()],
    );
    automation.flow.nodes[0].node_id = "refine_results".to_string();
    automation.flow.nodes[0].objective = "Filter and compare the gathered findings.".to_string();

    assert!(repair_automation_output_contracts(&mut automation));

    let draft = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "draft_deliverable")
        .expect("draft node");
    assert_eq!(draft.input_refs.len(), 1);
    assert_eq!(draft.input_refs[0].from_step_id, "refine_results");
    assert_eq!(
        draft
            .output_contract
            .as_ref()
            .map(|contract| contract.kind.as_str()),
        Some("report_markdown")
    );
    assert!(draft
        .output_contract
        .as_ref()
        .is_some_and(|contract| contract.schema.is_none()));
    assert!(draft
        .output_contract
        .as_ref()
        .and_then(|contract| contract.summary_guidance.as_deref())
        .is_some_and(
            |guidance| guidance.contains("Read and synthesize the strongest upstream artifacts")
        ));

    let finalize = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "finalize_outputs")
        .expect("finalize node");
    assert_eq!(finalize.input_refs.len(), 1);
    assert_eq!(finalize.input_refs[0].from_step_id, "draft_deliverable");
    assert_eq!(
        finalize
            .output_contract
            .as_ref()
            .map(|contract| contract.kind.as_str()),
        Some("report_markdown")
    );
}

#[test]
fn repair_automation_output_contracts_handles_text_json_and_code_outputs() {
    let mut text_node = bare_node();
    text_node.node_id = "write_notes".to_string();
    text_node.objective = "Write the final plain text notes to reports/findings.txt.".to_string();
    text_node.depends_on = vec!["gather".to_string()];
    text_node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: Some(json!({"type": "object"})),
        summary_guidance: None,
    });
    text_node.metadata = Some(json!({"builder": {"output_path": "reports/findings.txt"}}));

    let mut json_node = bare_node();
    json_node.node_id = "export_json".to_string();
    json_node.objective = "Export the structured results to artifacts/findings.json.".to_string();
    json_node.depends_on = vec!["write_notes".to_string()];
    json_node.output_contract = Some(AutomationFlowOutputContract {
        kind: "generic_artifact".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    json_node.metadata = Some(json!({"builder": {"output_path": "artifacts/findings.json"}}));

    let mut code_node = bare_node();
    code_node.node_id = "render_yaml".to_string();
    code_node.objective =
        "Render the final workflow config to config/agent-workflow.yaml.".to_string();
    code_node.depends_on = vec!["export_json".to_string()];
    code_node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: Some(json!({"type": "object"})),
        summary_guidance: None,
    });
    code_node.metadata = Some(json!({"builder": {"output_path": "config/agent-workflow.yaml"}}));

    let mut automation = automation_with_output_targets(
        vec![bare_node(), text_node, json_node, code_node],
        vec![
            "reports/findings.txt".to_string(),
            "artifacts/findings.json".to_string(),
            "config/agent-workflow.yaml".to_string(),
        ],
    );
    automation.flow.nodes[0].node_id = "gather".to_string();
    automation.flow.nodes[0].objective = "Gather the source evidence.".to_string();

    assert!(repair_automation_output_contracts(&mut automation));

    let text_node = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "write_notes")
        .expect("text node");
    assert_eq!(
        text_node
            .output_contract
            .as_ref()
            .map(|contract| contract.kind.as_str()),
        Some("text_summary")
    );
    assert_eq!(text_node.input_refs.len(), 1);

    let json_node = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "export_json")
        .expect("json node");
    assert_eq!(
        json_node
            .output_contract
            .as_ref()
            .map(|contract| contract.kind.as_str()),
        Some("structured_json")
    );
    assert_eq!(
        json_node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.validator),
        Some(crate::AutomationOutputValidatorKind::StructuredJson)
    );

    let code_node = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "render_yaml")
        .expect("code node");
    assert_eq!(
        code_node
            .output_contract
            .as_ref()
            .map(|contract| contract.kind.as_str()),
        Some("code_patch")
    );
    assert_eq!(
        code_node
            .output_contract
            .as_ref()
            .and_then(|contract| contract.validator),
        Some(crate::AutomationOutputValidatorKind::CodePatch)
    );
    assert!(code_node
        .output_contract
        .as_ref()
        .is_some_and(|contract| contract.schema.is_none()));
    assert_eq!(code_node.input_refs.len(), 1);
}

#[test]
fn repair_automation_output_contracts_preserves_specialized_contracts() {
    let mut node = bare_node();
    node.node_id = "final_brief".to_string();
    node.objective = "Write the final brief to reports/final-brief.md.".to_string();
    node.depends_on = vec!["research".to_string()];
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "brief".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
        enforcement: None,
        schema: Some(json!({"type": "object"})),
        summary_guidance: None,
    });
    node.metadata = Some(json!({"builder": {"output_path": "reports/final-brief.md"}}));

    let mut automation = automation_with_output_targets(
        vec![bare_node(), node],
        vec!["reports/final-brief.md".to_string()],
    );
    automation.flow.nodes[0].node_id = "research".to_string();
    automation.flow.nodes[0].objective = "Research the topic.".to_string();

    assert!(repair_automation_output_contracts(&mut automation));

    let node = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "final_brief")
        .expect("brief node");
    assert_eq!(
        node.output_contract
            .as_ref()
            .map(|contract| contract.kind.as_str()),
        Some("brief")
    );
    assert_eq!(
        node.output_contract
            .as_ref()
            .and_then(|contract| contract.validator),
        Some(crate::AutomationOutputValidatorKind::ResearchBrief)
    );
    assert_eq!(node.input_refs.len(), 1);
    assert_eq!(node.input_refs[0].from_step_id, "research");
}

#[test]
fn canonicalize_automation_output_paths_rewrites_legacy_timestamp_templates() {
    let mut node = bare_node();
    node.node_id = "finalize_outputs".to_string();
    node.objective = "Write the final report.".to_string();
    node.metadata = Some(json!({
        "builder": {
            "output_path": "reports/agent_automation_painpoints_YYYY-MM-DD_HH-MM-SS.md",
            "output_files": [
                "reports/agent_automation_painpoints_YYYY-MM-DD_HH-MM-SS.md",
                "reports/index_YYYY-MM-DD_HHMM.json"
            ],
            "must_write_files": ["reports/{{date}}-summary.md"]
        },
        "studio": {
            "output_path": "reports/agent_automation_painpoints_YYYY-MM-DD_HH-MM-SS.md",
            "output_files": ["reports/index_YYYY-MM-DD_HHMM.json"]
        }
    }));

    let mut automation = automation_with_output_targets(
        vec![node],
        vec![
            "reports/agent_automation_painpoints_YYYY-MM-DD_HH-MM-SS.md".to_string(),
            "reports/index_YYYY-MM-DD_HHMM.json".to_string(),
            "reports/{{date}}-summary.md".to_string(),
        ],
    );

    assert!(canonicalize_automation_output_paths(&mut automation));
    assert_eq!(
        automation.output_targets,
        vec![
            "reports/agent_automation_painpoints_{current_timestamp_filename}.md",
            "reports/index_{current_date}_{current_time}.json",
            "reports/{current_date}-summary.md",
        ]
    );

    let metadata = automation.flow.nodes[0]
        .metadata
        .as_ref()
        .expect("node metadata");
    assert_eq!(
        metadata
            .get("builder")
            .and_then(|builder| builder.get("output_path"))
            .and_then(|value| value.as_str()),
        Some("reports/agent_automation_painpoints_{current_timestamp_filename}.md")
    );
    assert_eq!(
        metadata
            .get("studio")
            .and_then(|studio| studio.get("output_path"))
            .and_then(|value| value.as_str()),
        Some("reports/agent_automation_painpoints_{current_timestamp_filename}.md")
    );
    assert_eq!(
        metadata
            .get("builder")
            .and_then(|builder| builder.get("output_files"))
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|entry| entry.as_str())
                    .collect::<Vec<_>>()
            }),
        Some(vec![
            "reports/agent_automation_painpoints_{current_timestamp_filename}.md",
            "reports/index_{current_date}_{current_time}.json",
        ])
    );
    assert_eq!(
        metadata
            .get("builder")
            .and_then(|builder| builder.get("must_write_files"))
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|entry| entry.as_str())
                    .collect::<Vec<_>>()
            }),
        Some(vec!["reports/{current_date}-summary.md"])
    );
}

#[test]
fn canonicalize_automation_output_paths_leaves_clean_paths_alone() {
    let mut automation = automation_with_output_targets(
        vec![bare_node()],
        vec!["reports/{current_timestamp_filename}.md".to_string()],
    );

    assert!(!canonicalize_automation_output_paths(&mut automation));
}

#[test]
fn knowledge_task_family_prefers_explicit_override() {
    let mut node = bare_node();
    node.metadata = Some(json!({
        "builder": {
            "task_family": "Support / Ops"
        }
    }));

    assert_eq!(automation_node_knowledge_task_family(&node), "support-ops");
}

#[test]
fn knowledge_task_family_groups_equivalent_code_workflows() {
    let code = code_workflow_node();
    let repo_fix = repo_fix_workflow_node();

    assert_eq!(automation_node_knowledge_task_family(&code), "code");
    assert_eq!(automation_node_knowledge_task_family(&repo_fix), "code");

    let code_key = tandem_orchestrator::build_knowledge_coverage_key(
        "project-1",
        Some("engineering/debugging"),
        &automation_node_knowledge_task_family(&code),
        "startup race",
    );
    let repo_fix_key = tandem_orchestrator::build_knowledge_coverage_key(
        "project-1",
        Some("engineering/debugging"),
        &automation_node_knowledge_task_family(&repo_fix),
        "startup race",
    );

    assert_eq!(code_key, repo_fix_key);
}

#[test]
fn task_kind_falls_back_to_task_class() {
    let node = task_class_only_node();

    assert_eq!(
        automation_node_task_kind(&node).as_deref(),
        Some("code_change")
    );
    assert!(automation_node_is_code_workflow(&node));
}

#[test]
fn knowledge_task_family_uses_workflow_class_for_research_briefs() {
    let research = research_brief_node();
    assert_eq!(automation_node_knowledge_task_family(&research), "research");
}

#[test]
fn connector_backed_intent_surfaces_mcp_discovery() {
    let mut node = bare_node();
    node.objective = "Research Reddit threads about AI assistants.".to_string();

    assert!(automation_node_should_surface_mcp_discovery(
        &node,
        &Vec::new()
    ));
    assert!(automation_node_should_surface_mcp_discovery(
        &bare_node(),
        &vec!["github".to_string()]
    ));
    assert!(!automation_node_should_surface_mcp_discovery(
        &bare_node(),
        &Vec::new()
    ));
}

#[test]
fn mcp_list_is_only_added_when_servers_are_selected() {
    let requested = automation_add_mcp_list_when_scoped(vec!["read".to_string()], false);
    assert_eq!(requested, vec!["read".to_string()]);

    let requested = automation_add_mcp_list_when_scoped(vec!["read".to_string()], true);
    assert!(requested.iter().any(|tool| tool == "mcp_list"));
}

#[test]
fn read_only_snapshot_rollback_guard_restores_mutated_file_on_drop() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-read-only-rollback-guard-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let workspace_root = workspace_root.to_str().expect("workspace root").to_string();

    let source_path = format!("{}/RESUME.md", workspace_root);
    let original = "Original resume content\n";
    std::fs::write(&source_path, original).expect("write source file");

    let mut snapshot = BTreeMap::new();
    snapshot.insert(
        "RESUME.md".to_string(),
        std::fs::read(&source_path).expect("snapshot read"),
    );

    {
        let _rollback = ReadOnlySourceSnapshotRollback::armed(&workspace_root, &snapshot);
        std::fs::write(&source_path, "workflow mutated source file").expect("mutate source file");
    }

    let restored = std::fs::read_to_string(&source_path).expect("restore source file");
    assert_eq!(restored, original);

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn connector_backed_publish_node_requests_named_server_mcp_tools() {
    let mut node = bare_node();
    node.objective =
        "Use blog-mcp to inspect the publishing flow and submit the draft.".to_string();
    node.metadata = Some(json!({
        "builder": {
            "prompt": "Create or update the post with blog-mcp and submit it for review."
        }
    }));

    let requested = automation_requested_server_scoped_mcp_tools(
        &node,
        &["blog-mcp".to_string(), "tandem-mcp".to_string()],
    );

    assert_eq!(requested, vec!["mcp.blog_mcp.*".to_string()]);
}

#[test]
fn server_scoped_mcp_patterns_expand_into_concrete_tools() {
    let available_tool_names = std::collections::HashSet::from([
        "mcp.blog_mcp.create_blog_draft".to_string(),
        "mcp.blog_mcp.submit_blog_for_review".to_string(),
        "mcp.tandem_mcp.search_docs".to_string(),
        "read".to_string(),
    ]);

    let effective = automation_expand_effective_offered_tools(
        &["read".to_string(), "mcp.blog_mcp.*".to_string()],
        &available_tool_names,
    );

    assert!(effective
        .iter()
        .any(|tool| tool == "mcp.blog_mcp.create_blog_draft"));
    assert!(effective
        .iter()
        .any(|tool| tool == "mcp.blog_mcp.submit_blog_for_review"));
    assert!(!effective
        .iter()
        .any(|tool| tool == "mcp.tandem_mcp.search_docs"));
}

#[test]
fn handoff_only_structured_json_removes_write_tool() {
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });

    let requested = normalize_automation_requested_tools(
        &node,
        "/tmp",
        vec!["read".to_string(), "write".to_string()],
    );

    assert!(requested.iter().any(|tool| tool == "read"));
    assert!(!requested
        .iter()
        .any(|tool| matches!(tool.as_str(), "write" | "edit" | "apply_patch")));
}

#[test]
fn structured_json_with_declared_output_keeps_write_tool() {
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": "artifacts/assess.json"
        }
    }));

    let requested = normalize_automation_requested_tools(&node, "/tmp", vec!["read".to_string()]);

    assert!(requested.iter().any(|tool| tool == "write"));
}

#[test]
fn bootstrap_inference_skips_optional_slash_separated_input_files() {
    let mut node = bare_node();
    node.node_id = "collect_inputs".to_string();
    node.objective = "Initialize any missing job-search workspace directories and files, read README.md/AGENTS.md/RESUME.md if present, and update resume-overview.md, resume-positioning.md, resume-state.json, sources/search-targets.md, tracker/search-ledger/2026-04-09.json, tracker/seen-jobs.jsonl, tracker/pipeline.md, and daily-recaps/2026-04-09-job-search-recap.md as needed before any search begins.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: Some("Return a structured handoff.".to_string()),
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/collect-inputs.json"
        }
    }));

    let must_write_files = automation_node_must_write_files(&node);

    assert!(!must_write_files.iter().any(|path| path == "README.md"));
    assert!(!must_write_files.iter().any(|path| path == "AGENTS.md"));
    assert!(!must_write_files.iter().any(|path| path == "RESUME.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "resume-overview.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "tracker/pipeline.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "daily-recaps/2026-04-09-job-search-recap.md"));
}

#[test]
fn bootstrap_inference_skips_read_only_source_of_truth_files() {
    let mut node = bare_node();
    node.node_id = "collect_inputs".to_string();
    node.objective = "Read RESUME.md as the source of truth for skills, role targets, and geography preferences. If resume_overview.md does not exist, create it. Create or append daily_results_2026-04-15.md in the workspace root and keep RESUME.md untouched.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: Some("Return a structured handoff.".to_string()),
    });

    let must_write_files = automation_node_must_write_files(&node);

    assert!(!must_write_files.iter().any(|path| path == "RESUME.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "resume_overview.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "daily_results_2026-04-15.md"));
}

#[test]
fn source_of_truth_files_disable_optional_workspace_reads() {
    let mut node = bare_node();
    node.objective = "Read RESUME.md as the source of truth for skills, role targets, and geography preferences. If resume_overview.md does not exist, create it. Create or append daily_results_2026-04-15.md in the workspace root and keep RESUME.md untouched.".to_string();

    assert!(!automation_node_allows_optional_workspace_reads(&node));
}

#[test]
fn required_source_read_paths_focus_on_exact_named_source_files() {
    let mut node = bare_node();
    node.node_id = "assess".to_string();
    node.objective = "Analyze the local RESUME.md file and use it as the source of truth for skills. Never edit, rewrite, rename, move, or delete RESUME.md. If resume_overview.md is missing, create it.".to_string();
    node.metadata = Some(json!({
        "builder": {
            "input_files": ["/home/evan/job-hunt/RESUME.md"],
            "output_files": ["resume_overview.md", "daily_results_{current_date}.md"]
        }
    }));
    let automation = AutomationV2Spec {
        automation_id: "automation-source-reads".to_string(),
        name: "Source Reads".to_string(),
        description: Some("Only read from RESUME.md and keep it untouched.".to_string()),
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
            nodes: vec![node.clone()],
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

    let required_paths =
        super::enforcement::automation_node_required_source_read_paths_for_automation(
            &automation,
            &node,
            "/home/evan/job-hunt",
            Some(&runtime_values("2026-04-15", "1446", "2026-04-15 14:46")),
        );

    assert_eq!(required_paths, vec!["RESUME.md".to_string()]);
}

#[test]
fn required_source_read_paths_handles_punctuation_backticks_and_mixed_language() {
    let mut node = bare_node();
    node.node_id = "assess".to_string();
    node.objective = "Analyze the local `RESUME.md` file and use it as a source of truth for skills and roles. Never edit, rewrite, rename, move, or delete it. If resume_overview.md already exists, reuse it; otherwise create it from `RESUME.md`."
        .to_string();
    node.metadata = Some(json!({
        "builder": {
            "input_files": ["/home/evan/job-hunt/RESUME.md"],
            "output_files": ["resume_overview.md"]
        }
    }));

    let automation = AutomationV2Spec {
        automation_id: "automation-source-reads-quoted".to_string(),
        name: "Source Reads Quoted".to_string(),
        description: Some("Read from RESUME.md only.".to_string()),
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
            nodes: vec![node.clone()],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_tool_calls: None,
            max_total_runtime_ms: None,
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

    let required_paths =
        super::enforcement::automation_node_required_source_read_paths_for_automation(
            &automation,
            &node,
            "/home/evan/job-hunt",
            Some(&runtime_values("2026-04-15", "1500", "2026-04-15 15:00")),
        );

    assert_eq!(required_paths, vec!["RESUME.md".to_string()]);
}

#[test]
fn explicit_output_files_skip_read_only_source_of_truth_files() {
    let mut node = bare_node();
    node.node_id = "compare_with_features".to_string();
    node.objective = "Read RESUME.md as the source of truth for skills, role targets, and geography preferences. If resume_overview.md does not exist, create it. Create or append daily_results_2026-04-15.md in the workspace root and keep RESUME.md untouched.".to_string();
    node.metadata = Some(json!({
        "builder": {
            "output_files": [
                "RESUME.md",
                "resume_overview.md",
                "daily_results_2026-04-15.md"
            ]
        }
    }));

    let must_write_files = automation_node_must_write_files(&node);

    assert!(!must_write_files.iter().any(|path| path == "RESUME.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "resume_overview.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "daily_results_2026-04-15.md"));
}

#[test]
fn explicit_must_write_files_skip_read_only_source_of_truth_files() {
    let mut node = bare_node();
    node.node_id = "compare_with_features".to_string();
    node.objective = "Read RESUME.md as the source of truth for skills, role targets, and geography preferences. If resume_overview.md does not exist, create it. Create or append daily_results_2026-04-15.md in the workspace root and keep RESUME.md untouched.".to_string();
    node.metadata = Some(json!({
        "builder": {
            "must_write_files": [
                "RESUME.md",
                "resume_overview.md",
                "daily_results_2026-04-15.md"
            ]
        }
    }));

    let must_write_files = automation_node_must_write_files(&node);

    assert!(!must_write_files.iter().any(|path| path == "RESUME.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "resume_overview.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "daily_results_2026-04-15.md"));
}

#[test]
fn bootstrap_inference_applies_to_dependent_workspace_bootstrap_nodes() {
    let mut node = bare_node();
    node.node_id = "execute_goal".to_string();
    node.depends_on = vec!["collect_inputs".to_string()];
    node.objective = "Initialize any missing directories and files, create tracker/search-ledger/{current_date}.json and daily-recaps/{current_date}-job-search-recap.md, and update tracker/pipeline.md as needed.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: Some("Return a structured handoff.".to_string()),
    });

    let must_write_files = automation_node_must_write_files(&node);

    assert!(must_write_files
        .iter()
        .any(|path| path == "tracker/search-ledger/{current_date}.json"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "daily-recaps/{current_date}-job-search-recap.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "tracker/pipeline.md"));
}

#[test]
fn automation_output_targets_fill_in_final_node_workspace_writes() {
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
            "tracker/search-ledger/{current_date}.json".to_string(),
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
    let mut node = bare_node();
    node.node_id = "analyze_findings".to_string();
    node.depends_on = vec!["research_sources".to_string()];
    node.objective = "Normalize worthwhile jobs, update daily ranked opportunities, shortlist, and pipeline views, then merge the daily recap.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });

    let must_write_files = automation_node_must_write_files_for_automation(
        &automation,
        &node,
        Some(&runtime_values("2026-04-09", "1304", "2026-04-09 13:04")),
    );

    assert!(
        must_write_files.is_empty(),
        "must_write_files should only include node's own output_files, not automation output_targets. Got: {must_write_files:?}"
    );
    assert!(!must_write_files
        .iter()
        .any(|path| path.contains("daily-recaps")));
    assert!(!must_write_files
        .iter()
        .any(|path| path.contains("opportunities")));
    assert!(!must_write_files.iter().any(|path| path.contains("tracker")));
}

#[test]
fn automation_output_targets_replace_runtime_placeholders_before_dedup() {
    let automation = AutomationV2Spec {
        automation_id: "automation-runtime-dedup".to_string(),
        name: "Runtime Dedup".to_string(),
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
            "opportunities/raw/{current_date}/{current_time}-findings.json".to_string(),
            "opportunities/raw/{current_date}/{current_time}-findings.md".to_string(),
            "tracker/search-ledger/{current_date}.json".to_string(),
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
    let mut node = bare_node();
    node.node_id = "research_sources".to_string();
    node.objective = "Inspect tracker/search-ledger/2026-04-09.json, avoid duplicate work, and write raw findings immediately to opportunities/raw/2026-04-09/2138-findings.md and opportunities/raw/2026-04-09/2138-findings.json.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: Some("Return a structured handoff.".to_string()),
    });

    let must_write_files = automation_node_must_write_files_for_automation(
        &automation,
        &node,
        Some(&runtime_values("2026-04-09", "2138", "2026-04-09 21:38")),
    );

    assert_eq!(
        must_write_files
            .iter()
            .filter(|path| *path == "opportunities/raw/2026-04-09/2138-findings.json")
            .count(),
        1
    );
    assert_eq!(
        must_write_files
            .iter()
            .filter(|path| *path == "opportunities/raw/2026-04-09/2138-findings.md")
            .count(),
        1
    );
    assert_eq!(
        must_write_files
            .iter()
            .filter(|path| *path == "tracker/search-ledger/2026-04-09.json")
            .count(),
        1
    );
    assert!(!must_write_files
        .iter()
        .any(|path| path.contains("{current_date}") || path.contains("{current_time}")));
}

#[test]
fn intermediate_nodes_cannot_treat_live_output_targets_as_input_files() {
    let mut gather = bare_node();
    gather.node_id = "gather_fintech_candidates".to_string();
    gather.objective = "Research fintech sponsor candidates.".to_string();
    gather.metadata = Some(json!({
        "builder": {
            "input_files": ["/tmp/workspace/sales/genz-sponsor-research/2026-04-16_1530_genz_sponsor_targets.md"]
        }
    }));

    let mut finalize = bare_node();
    finalize.node_id = "draft_markdown_report".to_string();
    finalize.objective = "Write the final sponsor targets report to sales/genz-sponsor-research/2026-04-16_1530_genz_sponsor_targets.md.".to_string();
    finalize.depends_on = vec!["gather_fintech_candidates".to_string()];

    let automation = automation_with_live_output_target(vec![gather.clone(), finalize]);
    let runtime_values = runtime_values("2026-04-16", "1530", "2026-04-16 15:30");

    let input_files = automation_node_effective_input_files_for_automation(
        &automation,
        &gather,
        Some(&runtime_values),
    );

    assert!(input_files.is_empty(), "expected live output targets to be stripped from intermediate input files, got {input_files:?}");
}

#[test]
fn terminal_report_node_may_access_live_output_target() {
    let gather = {
        let mut node = bare_node();
        node.node_id = "gather_candidates".to_string();
        node.objective = "Research sponsor candidates.".to_string();
        node
    };
    let mut finalize = bare_node();
    finalize.node_id = "draft_markdown_report".to_string();
    finalize.objective = "Append the final sponsor targets report to sales/genz-sponsor-research/2026-04-16_1530_genz_sponsor_targets.md.".to_string();
    finalize.depends_on = vec!["gather_candidates".to_string()];
    finalize.metadata = Some(json!({
        "builder": {
            "input_files": ["sales/genz-sponsor-research/2026-04-16_1530_genz_sponsor_targets.md"]
        }
    }));

    let automation = automation_with_live_output_target(vec![gather, finalize.clone()]);
    let runtime_values = runtime_values("2026-04-16", "1530", "2026-04-16 15:30");

    let input_files = automation_node_effective_input_files_for_automation(
        &automation,
        &finalize,
        Some(&runtime_values),
    );

    assert_eq!(
        input_files,
        vec!["sales/genz-sponsor-research/2026-04-16_1530_genz_sponsor_targets.md".to_string()]
    );
}

#[test]
fn report_markdown_nodes_do_not_infer_template_filenames_as_workspace_writes() {
    let automation = AutomationV2Spec {
        automation_id: "automation-report-markdown".to_string(),
        name: "Report Markdown".to_string(),
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
    let mut node = bare_node();
    node.node_id = "analyze_findings".to_string();
    node.objective = "Normalize only worthwhile jobs into per-role folders with `source.md`, `normalized-job.md`, `fit-analysis.md`, `apply-details.md`, and `status.json`; score fit honestly using `RESUME.md`, `resume-overview.md`, and `resume-positioning.md`; update daily ranked opportunities, shortlist, and pipeline views; then merge the daily recap so ratings, links, company names, role titles, and concise next steps are present.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });

    let must_write_files = automation_node_must_write_files_for_automation(
        &automation,
        &node,
        Some(&runtime_values("2026-04-09", "2138", "2026-04-09 21:38")),
    );

    assert!(!must_write_files.iter().any(|path| {
        matches!(
            path.as_str(),
            "source.md"
                | "normalized-job.md"
                | "fit-analysis.md"
                | "apply-details.md"
                | "status.json"
                | "RESUME.md"
                | "resume-overview.md"
                | "resume-positioning.md"
        )
    }));
    assert!(must_write_files
        .iter()
        .any(|path| path == "daily-recaps/2026-04-09-job-search-recap.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "opportunities/ranked/2026-04-09-ranked-opportunities.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "opportunities/shortlisted/2026-04-09-shortlist.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "tracker/pipeline.md"));
}

#[test]
fn automation_wide_read_only_rules_filter_later_node_write_targets() {
    let protect_node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "assess".to_string(),
        agent_id: "a1".to_string(),
        objective: "Read RESUME.md as the source of truth. Never edit, rewrite, rename, move, or delete RESUME.md.".to_string(),
        depends_on: vec![],
        input_refs: vec![],
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    };
    let mut write_node = bare_node();
    write_node.node_id = "generate_report".to_string();
    write_node.objective =
        "Create the daily results file and return the append-safe report summary.".to_string();
    write_node.metadata = Some(json!({
        "builder": {
            "output_files": ["RESUME.md", "daily_results_{current_date}.md"]
        }
    }));
    let automation = AutomationV2Spec {
        automation_id: "automation-read-only-invariant".to_string(),
        name: "Read Only Invariant".to_string(),
        description: Some(
            "Only read from RESUME.md. Keep RESUME.md untouched throughout the workflow."
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
            nodes: vec![protect_node, write_node.clone()],
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

    let must_write_files = automation_node_must_write_files_for_automation(
        &automation,
        &write_node,
        Some(&runtime_values("2026-04-15", "1049", "2026-04-15 10:49")),
    );

    assert!(!must_write_files.iter().any(|path| path == "RESUME.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "daily_results_2026-04-15.md"));
}

#[test]
fn wildcard_tool_allowlist_does_not_select_mcp_servers() {
    let selected = automation_infer_selected_mcp_servers(
        &Vec::new(),
        &vec!["*".to_string()],
        &vec!["github".to_string(), "slack".to_string()],
        false,
    );
    assert!(selected.is_empty());
}

#[test]
fn automation_quality_mode_defaults_to_strict_and_requires_rollback_for_legacy_metadata() {
    let strict_mode =
        super::enforcement::automation_quality_mode_resolution_from_metadata(None, true, false);
    assert_eq!(
        strict_mode.effective,
        super::enforcement::AutomationQualityMode::StrictResearchV1
    );
    assert_eq!(strict_mode.requested, None);
    assert!(!strict_mode.legacy_rollback_enabled);

    let legacy_metadata = serde_json::json!({
        "quality_mode": "legacy"
    });
    let legacy_object = legacy_metadata.as_object().cloned().expect("object");
    let forced_strict = super::enforcement::automation_quality_mode_resolution_from_metadata(
        Some(&legacy_object),
        true,
        false,
    );
    assert_eq!(
        forced_strict.requested,
        Some(super::enforcement::AutomationQualityMode::Legacy)
    );
    assert_eq!(
        forced_strict.effective,
        super::enforcement::AutomationQualityMode::StrictResearchV1
    );

    let legacy_mode = super::enforcement::automation_quality_mode_resolution_from_metadata(
        Some(&legacy_object),
        true,
        true,
    );
    assert_eq!(
        legacy_mode.requested,
        Some(super::enforcement::AutomationQualityMode::Legacy)
    );
    assert_eq!(
        legacy_mode.effective,
        super::enforcement::AutomationQualityMode::Legacy
    );
}

// -----------------------------------------------------------------------
// automation_infer_selected_mcp_servers
// -----------------------------------------------------------------------

#[test]
fn mcp_servers_empty_inputs_returns_empty() {
    let result = automation_infer_selected_mcp_servers(&[], &[], &[], false);
    assert!(result.is_empty());
}

#[test]
fn mcp_servers_explicit_allowed_list_returned_directly() {
    let result = automation_infer_selected_mcp_servers(
        &["gmail".to_string()],
        &[],
        &["gmail".to_string(), "slack".to_string()],
        false,
    );
    assert_eq!(result, vec!["gmail"]);
}

#[test]
fn mcp_servers_allowlist_wildcard_does_not_select_any_servers() {
    let enabled = vec!["gmail".to_string(), "slack".to_string()];
    let result = automation_infer_selected_mcp_servers(&[], &["*".to_string()], &enabled, false);
    assert!(result.is_empty());
}

#[test]
fn mcp_servers_requires_email_delivery_returns_all_enabled() {
    let enabled = vec!["gmail".to_string(), "hubspot".to_string()];
    let result = automation_infer_selected_mcp_servers(&[], &[], &enabled, true);
    assert_eq!(result, enabled);
}

#[test]
fn report_markdown_preserves_full_upstream_inputs() {
    let node = report_markdown_node();
    assert!(automation_node_preserves_full_upstream_inputs(&node));

    let mut email_delivery = email_delivery_node();
    email_delivery.depends_on = vec!["generate_report".to_string()];
    email_delivery.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "generate_report".to_string(),
        alias: "report_body".to_string(),
    }];
    assert!(automation_node_preserves_full_upstream_inputs(
        &email_delivery
    ));

    let mut execute_goal = bare_node();
    execute_goal.node_id = "execute_goal".to_string();
    execute_goal.objective =
        "Create a Gmail draft or send the final HTML summary email to recipient@example.com if mail tools are available.".to_string();
    execute_goal.output_contract = Some(AutomationFlowOutputContract {
        kind: "approval_gate".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    execute_goal.depends_on = vec!["generate_report".to_string()];
    execute_goal.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "generate_report".to_string(),
        alias: "report_body".to_string(),
    }];
    execute_goal.metadata = Some(json!({
        "delivery": {
            "method": "email",
            "to": "recipient@example.com",
            "content_type": "text/html",
            "inline_body_only": true,
            "attachments": false
        }
    }));
    assert!(automation_node_preserves_full_upstream_inputs(
        &execute_goal
    ));

    let mut text_summary = bare_node();
    text_summary.output_contract = Some(AutomationFlowOutputContract {
        kind: "text_summary".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    text_summary.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "prev".to_string(),
        alias: "input".to_string(),
    }];
    assert!(automation_node_preserves_full_upstream_inputs(
        &text_summary
    ));
}

#[test]
fn blog_draft_objective_with_negative_gmail_mentions_does_not_require_email_delivery() {
    let mut node = report_markdown_node();
    node.node_id = "generate_report".to_string();
    node.objective = "The blog post is NOT about Gmail/Reddit/blog integrations as product marketing. Before drafting the article, write article-thesis.md, then produce blog-draft.md and blog-package.md with a publish-ready article.".to_string();

    assert!(!automation_node_requires_email_delivery(&node));
}

#[test]
fn explicit_gmail_draft_objective_requires_email_delivery() {
    let mut node = bare_node();
    node.node_id = "execute_goal".to_string();
    node.objective =
        "Create a Gmail draft or send the final HTML summary email to recipient@example.com."
            .to_string();

    assert!(automation_node_requires_email_delivery(&node));
}

#[test]
fn generic_synthesis_nodes_get_default_artifact_paths_without_legacy_ids() {
    let node = generic_research_artifact_node();

    assert_eq!(
        super::node_runtime_impl::automation_node_default_output_path(&node).as_deref(),
        Some(".tandem/artifacts/summarize-resume-signals.json")
    );
}

#[test]
fn delivery_nodes_do_not_get_default_artifact_paths() {
    let node = email_delivery_node();

    assert_eq!(
        super::node_runtime_impl::automation_node_default_output_path(&node),
        None
    );
}

#[test]
fn mcp_servers_allowlist_namespace_pattern_matches_server() {
    // "mcp.my_server.*" should match server named "my-server" (dashes → underscores)
    let enabled = vec!["my-server".to_string(), "other".to_string()];
    let result = automation_infer_selected_mcp_servers(
        &[],
        &["mcp.my_server.*".to_string()],
        &enabled,
        false,
    );
    assert_eq!(result, vec!["my-server"]);
}

#[test]
fn mcp_servers_deduplicates_when_allowed_and_allowlist_overlap() {
    let enabled = vec!["gmail".to_string()];
    let result = automation_infer_selected_mcp_servers(
        &["gmail".to_string()],
        &["mcp.gmail.*".to_string()],
        &enabled,
        false,
    );
    assert_eq!(result, vec!["gmail"]);
}

#[test]
fn missing_capabilities_from_collapsed_tool_resolution_are_detected() {
    let node = email_delivery_node();
    let available_tool_names = std::collections::HashSet::from(["mcp_list".to_string()]);
    let resolution = automation_resolve_capabilities_with_schemas(
        &node,
        "structured_json",
        &["mcp_list".to_string()],
        &available_tool_names,
        &[],
    );

    assert_eq!(
        automation_capability_resolution_missing_capabilities(&resolution),
        vec!["email_draft".to_string(), "email_send".to_string()]
    );
}

#[test]
fn retry_attempt_tool_failure_labels_are_cleared_before_reuse() {
    let mut tool_telemetry = json!({
        "latest_web_research_failure": "web research timed out",
        "latest_email_delivery_failure": "smtp unauthorized",
        "attempt_evidence": {
            "evidence": {
                "web_research": {
                    "latest_failure": "dns error"
                }
            },
            "delivery": {
                "latest_failure": "unauthorized"
            }
        }
    });

    automation_reset_attempt_tool_failure_labels(&mut tool_telemetry);

    assert!(tool_telemetry
        .get("latest_web_research_failure")
        .is_some_and(Value::is_null));
    assert!(tool_telemetry
        .get("latest_email_delivery_failure")
        .is_some_and(Value::is_null));
    assert!(tool_telemetry
        .pointer("/attempt_evidence/evidence/web_research/latest_failure")
        .is_some_and(Value::is_null));
    assert!(tool_telemetry
        .pointer("/attempt_evidence/delivery/latest_failure")
        .is_some_and(Value::is_null));
}

#[test]
fn email_send_detection_recognizes_compact_sendemail_names() {
    assert!(automation_tool_name_is_email_send(
        "mcp.composio_1.gmail_sendemail"
    ));
    assert!(automation_tool_name_is_email_send("Gmail_SendEmail"));
    assert!(automation_tool_name_is_email_draft(
        "mcp.composio_1.gmail_draftemail"
    ));
}

#[test]
fn step_cost_provenance_marks_budget_limit_and_cost_deltas() {
    let provenance = automation_step_cost_provenance(
        "step_1",
        Some("gpt-5.1".to_string()),
        120,
        80,
        2.75,
        9.50,
        true,
    );

    assert_eq!(
        provenance.get("step_id").and_then(Value::as_str),
        Some("step_1")
    );
    assert_eq!(
        provenance.get("model_id").and_then(Value::as_str),
        Some("gpt-5.1")
    );
    assert_eq!(
        provenance.get("tokens_in").and_then(Value::as_u64),
        Some(120)
    );
    assert_eq!(
        provenance.get("tokens_out").and_then(Value::as_u64),
        Some(80)
    );
    assert_eq!(
        provenance.get("computed_cost_usd").and_then(Value::as_f64),
        Some(2.75)
    );
    assert_eq!(
        provenance
            .get("cumulative_run_cost_usd_at_step_end")
            .and_then(Value::as_f64),
        Some(9.50)
    );
    assert_eq!(
        provenance
            .get("budget_limit_reached")
            .and_then(Value::as_bool),
        Some(true)
    );
}

// -----------------------------------------------------------------------
// automation_tool_capability_ids
// -----------------------------------------------------------------------

#[test]
fn capability_ids_bare_node_empty() {
    let node = bare_node();
    let caps = automation_tool_capability_ids(&node, "research");
    assert!(
        caps.is_empty(),
        "bare node should yield no capabilities, got: {caps:?}"
    );
}

#[test]
fn capability_ids_node_with_input_ref_includes_workspace_read() {
    let node = node_with_input_ref();
    let caps = automation_tool_capability_ids(&node, "research");
    assert!(caps.contains(&"workspace_read".to_string()));
}

#[test]
fn capability_ids_code_workflow_git_patch_includes_verify_command() {
    let caps = automation_tool_capability_ids(&code_workflow_node(), "git_patch");
    assert!(
        caps.contains(&"verify_command".to_string()),
        "git_patch code node should require verify_command, got: {caps:?}"
    );
}

#[test]
fn capability_ids_code_workflow_research_mode_excludes_verify_command() {
    let caps = automation_tool_capability_ids(&code_workflow_node(), "research");
    assert!(
        !caps.contains(&"verify_command".to_string()),
        "research mode should not include verify_command, got: {caps:?}"
    );
}

#[test]
fn code_patch_contract_is_treated_as_a_code_workflow() {
    let node = code_patch_contract_node();
    assert_eq!(
        automation_output_validator_kind(&node),
        crate::AutomationOutputValidatorKind::CodePatch
    );
    assert!(automation_node_is_code_workflow(&node));
    assert_eq!(
        automation_node_execution_policy(&node, ".")
            .get("workflow_class")
            .and_then(Value::as_str),
        Some("code")
    );
}

#[test]
fn code_patch_contract_includes_verification_command_capability() {
    let caps = automation_tool_capability_ids(&code_patch_contract_node(), "git_patch");
    assert!(
        caps.contains(&"verify_command".to_string()),
        "code_patch contract should require verify_command in patch mode, got: {caps:?}"
    );
}

#[test]
fn code_patch_contract_enforcement_defaults_require_reads_and_prewrite_gates() {
    let enforcement = automation_node_output_enforcement(&code_patch_contract_node());
    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("code_change")
    );
    assert!(enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert!(enforcement
        .required_evidence
        .iter()
        .any(|value| value == "local_source_reads"));
    assert!(enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "workspace_inspection"));
    assert!(enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "concrete_reads"));
}

#[test]
fn code_patch_contract_requires_verification_before_completion() {
    let node = code_patch_contract_node();
    let tool_telemetry = json!({
        "verification_expected": true,
        "verification_ran": false
    });
    assert_eq!(
        detect_automation_node_failure_kind(&node, "blocked", None, None, None).as_deref(),
        None
    );
    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            "blocked",
            Some(false),
            None,
            Some(&json!({"verification_expected": true, "verification_ran": false}))
        )
        .as_deref(),
        Some("verification_missing")
    );
    assert_eq!(
        detect_automation_blocker_category(&node, "blocked", None, &tool_telemetry, None,),
        Some("verification_required".to_string())
    );
}

#[test]
fn local_citations_contract_defaults_to_local_research_not_external_research() {
    let enforcement = automation_node_output_enforcement(&local_citations_contract_node());
    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("local_research")
    );
    assert!(enforcement.required_tools.iter().any(|tool| tool == "glob"));
    assert!(enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert!(enforcement
        .required_evidence
        .iter()
        .any(|value| value == "local_source_reads"));
    assert!(enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "workspace_inspection"));
}

#[test]
fn mcp_citations_contract_defaults_to_artifact_only_without_local_read_gates() {
    let enforcement = automation_node_output_enforcement(&mcp_citations_contract_node());
    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("artifact_only")
    );
    assert!(!enforcement.required_tools.iter().any(|tool| tool == "glob"));
    assert!(!enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert!(!enforcement
        .required_evidence
        .iter()
        .any(|value| value == "local_source_reads"));
    assert!(!enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "workspace_inspection"));
    assert_eq!(enforcement.session_text_recovery.as_deref(), Some("allow"));
}

#[test]
fn auto_cleaned_marker_file_rejection_is_downgraded_when_output_is_valid() {
    assert!(super::should_downgrade_auto_cleaned_marker_rejection(
        Some("undeclared marker files created: .tandem_ack"),
        true,
        None,
        true
    ));
    assert!(!super::should_downgrade_auto_cleaned_marker_rejection(
        Some("undeclared marker files created: .tandem_ack"),
        false,
        None,
        true
    ));
    assert!(!super::should_downgrade_auto_cleaned_marker_rejection(
        Some("undeclared marker files created: .tandem_ack"),
        true,
        Some("no_concrete_reads"),
        true
    ));
    assert!(!super::should_downgrade_auto_cleaned_marker_rejection(
        Some("other rejection"),
        true,
        None,
        true
    ));
}

#[test]
fn capability_ids_output_is_sorted_and_deduplicated() {
    let node = node_with_input_ref();
    let caps = automation_tool_capability_ids(&node, "research");
    let mut sorted = caps.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        caps, sorted,
        "capability ids must be sorted and deduplicated"
    );
}

#[test]
fn capability_resolution_expands_wildcard_offered_email_tools() {
    let node = email_delivery_node();
    let available_tool_names = [
        "read".to_string(),
        "glob".to_string(),
        "mcp.composio_1.gmail_send_email".to_string(),
        "mcp.composio_1.gmail_create_email_draft".to_string(),
    ]
    .into_iter()
    .collect::<std::collections::HashSet<_>>();
    let resolution = automation_resolve_capabilities(
        &node,
        "artifact_write",
        &["mcp.composio_1.*".to_string()],
        &available_tool_names,
    );

    let offered_send_tools = resolution
        .get("email_tool_diagnostics")
        .and_then(|value| value.get("offered_send_tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let offered_draft_tools = resolution
        .get("email_tool_diagnostics")
        .and_then(|value| value.get("offered_draft_tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    assert!(offered_send_tools
        .iter()
        .any(|value| { value.as_str() == Some("mcp.composio_1.gmail_send_email") }));
    assert!(offered_draft_tools
        .iter()
        .any(|value| { value.as_str() == Some("mcp.composio_1.gmail_create_email_draft") }));
}

#[test]
fn capability_resolution_uses_metadata_for_unknown_tool_names() {
    let node = code_patch_contract_node();
    let available_tool_schemas = vec![
        ToolSchema::new("workspace_inspector", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Read)
                .domain(ToolDomain::Workspace)
                .reads_workspace(),
        ),
        ToolSchema::new("workspace_searcher", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Search)
                .domain(ToolDomain::Workspace)
                .reads_workspace()
                .preferred_for_discovery(),
        ),
        ToolSchema::new("workspace_writer", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Write)
                .domain(ToolDomain::Workspace)
                .writes_workspace()
                .requires_verification(),
        ),
        ToolSchema::new("run_local_checks", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Execute)
                .domain(ToolDomain::Shell),
        ),
    ];
    let available_tool_names = available_tool_schemas
        .iter()
        .map(|schema| schema.name.clone())
        .collect::<std::collections::HashSet<_>>();
    let resolution = automation_resolve_capabilities_with_schemas(
        &node,
        "git_patch",
        &available_tool_names.iter().cloned().collect::<Vec<_>>(),
        &available_tool_names,
        &available_tool_schemas,
    );

    assert_eq!(
        resolution["resolved"]["workspace_read"]["status"].as_str(),
        Some("resolved")
    );
    assert_eq!(
        resolution["resolved"]["workspace_discover"]["status"].as_str(),
        Some("resolved")
    );
    assert_eq!(
        resolution["resolved"]["artifact_write"]["status"].as_str(),
        Some("resolved")
    );
    assert_eq!(
        resolution["resolved"]["verify_command"]["status"].as_str(),
        Some("resolved")
    );
}

// -----------------------------------------------------------------------
// normalize_upstream_research_output_paths
// -----------------------------------------------------------------------

#[test]
fn normalize_upstream_paths_passthrough_when_no_content_key() {
    let output = json!({ "summary": "hello" });
    let result = normalize_upstream_research_output_paths("/workspace", None, &output);
    assert_eq!(
        result, output,
        "output with no 'content' key should be returned unchanged"
    );
}

#[test]
fn normalize_upstream_paths_survives_empty_handoff() {
    let output = json!({
        "content": {
            "text": "some text",
            "structured_handoff": {}
        }
    });
    let result = normalize_upstream_research_output_paths("/workspace", None, &output);
    assert!(result.is_object(), "result should still be a JSON object");
}

#[test]
fn normalize_upstream_paths_scopes_tandem_artifacts_for_run() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-upstream-run-scoped-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join(".tandem/runs/run-123/artifacts"))
        .expect("create artifacts");
    std::fs::write(
        workspace_root.join(".tandem/runs/run-123/artifacts/report.md"),
        "report",
    )
    .expect("write artifact");
    let output = json!({
        "content": {
            "structured_handoff": {
                "files_reviewed": [".tandem/artifacts/report.md"]
            }
        }
    });
    let result = normalize_upstream_research_output_paths(
        workspace_root.to_str().expect("workspace"),
        Some("run-123"),
        &output,
    );
    assert_eq!(
        result.pointer("/content/structured_handoff/files_reviewed/0"),
        Some(&json!(".tandem/runs/run-123/artifacts/report.md"))
    );
    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn required_output_path_scopes_shared_artifacts_for_run() {
    let mut node = bare_node();
    node.node_id = "generate_report".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/generate-report.md"
        }
    }));

    assert_eq!(
        automation_node_required_output_path_for_run(&node, Some("run-iso")),
        Some(".tandem/runs/run-iso/artifacts/generate-report.md".to_string())
    );
    assert_eq!(
        automation_node_required_output_path_for_run(&node, None),
        Some(".tandem/artifacts/generate-report.md".to_string())
    );
}

#[test]
fn required_output_path_with_runtime_resolves_legacy_timestamp_templates() {
    let mut node = bare_node();
    node.node_id = "finalize_outputs".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": "reports/agent_automation_painpoints_YYYY-MM-DD_HH-MM-SS.md"
        }
    }));

    assert_eq!(
        automation_node_required_output_path_with_runtime_for_run(
            &node,
            Some("run-ts"),
            Some(&runtime_values("2026-04-17", "1024", "2026-04-17 10:24")),
        ),
        Some("reports/agent_automation_painpoints_2026-04-17_10-24-00.md".to_string())
    );
}

#[test]
fn runtime_placeholder_replace_supports_legacy_timestamp_tokens() {
    let replaced = automation_runtime_placeholder_replace(
        "reports/run_YYYY-MM-DD_HH-MM-SS.md and logs/YYYYMMDD_HHMMSS.json",
        Some(&runtime_values("2026-04-17", "1024", "2026-04-17 10:24")),
    );

    assert_eq!(
        replaced,
        "reports/run_2026-04-17_10-24-00.md and logs/20260417_102400.json"
    );
}

#[test]
fn session_write_materialized_output_detects_run_scoped_artifact_files() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-current-attempt-output-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-123";
    let artifact_path = workspace_root.join(".tandem/runs/run-123/artifacts/report.md");
    std::fs::create_dir_all(
        artifact_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create artifacts dir");
    std::fs::write(&artifact_path, "report body").expect("write artifact");

    let mut session = Session::new(Some("write evidence".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": ".tandem/artifacts/report.md",
                "content": "report body"
            }),
            result: Some(json!({"output": "written"})),
            error: None,
        }],
    ));

    assert!(session_write_materialized_output_for_output(
        &session,
        workspace_root.to_str().expect("workspace root"),
        ".tandem/artifacts/report.md",
        Some(run_id),
        None,
    ));

    std::fs::remove_file(&artifact_path).expect("remove artifact");

    assert!(!session_write_materialized_output_for_output(
        &session,
        workspace_root.to_str().expect("workspace root"),
        ".tandem/artifacts/report.md",
        Some(run_id),
        None,
    ));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_waits_for_late_file_visibility() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-verified-output-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-reconcile";
    let output_path = ".tandem/artifacts/report.md";
    let resolved_path = workspace_root.join(".tandem/runs/run-reconcile/artifacts/report.md");
    std::fs::create_dir_all(
        resolved_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create artifacts dir");

    let mut session = Session::new(Some("reconcile visibility".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": output_path,
                "content": "report body"
            }),
            result: Some(json!({"output": "written"})),
            error: None,
        }],
    ));

    let writer_root = workspace_root.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        std::fs::write(
            writer_root.join(".tandem/runs/run-reconcile/artifacts/report.md"),
            "report body",
        )
        .expect("write delayed artifact");
    });

    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "generate_report".to_string(),
            agent_id: "writer".to_string(),
            objective: "Generate report".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        300,
        25,
    )
    .await
    .expect("resolve after delay");

    writer.join().expect("writer thread");
    assert_eq!(resolved.map(|value| value.path), Some(resolved_path));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_times_out_when_file_never_appears() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-verified-output-timeout-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-timeout";
    let output_path = ".tandem/artifacts/report.md";
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let session = Session::new(Some("reconcile timeout".to_string()), None);
    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "generate_report".to_string(),
            agent_id: "writer".to_string(),
            objective: "Generate report".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        50,
        10,
    )
    .await
    .expect("resolve timeout");

    assert!(resolved.is_none());

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_marks_stale_existing_run_output_as_not_current_attempt() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-stale-existing-output-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-stale-existing";
    let output_path = ".tandem/artifacts/report.md";
    let resolved_path = workspace_root.join(".tandem/runs/run-stale-existing/artifacts/report.md");
    std::fs::create_dir_all(
        resolved_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create artifacts dir");
    std::fs::write(&resolved_path, "stale report").expect("write stale artifact");

    let session = Session::new(Some("no output write this attempt".to_string()), None);
    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "generate_report".to_string(),
            agent_id: "writer".to_string(),
            objective: "Generate report".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        50,
        10,
    )
    .await
    .expect("resolve stale output")
    .expect("stale output should still resolve");

    assert_eq!(resolved.path, resolved_path);
    assert!(!resolved.materialized_by_current_attempt);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_recovers_json_artifact_from_session_text() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-session-text-json-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-session-json";
    let output_path = ".tandem/artifacts/research-sources.json";
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut session = Session::new(Some("session text recovery".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::Text {
            text: "{\n  \"sources\": [\n    {\n      \"path\": \"README.md\",\n      \"reason\": \"project overview\"\n    }\n  ],\n  \"summary\": \"Primary local sources identified.\"\n}\n{\"status\":\"completed\"}".to_string(),
        }],
    ));

    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "research_sources".to_string(),
            agent_id: "researcher".to_string(),
            objective: "Find and record local sources".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        50,
        10,
    )
    .await
    .expect("recover from session text");

    let expected =
        workspace_root.join(".tandem/runs/run-session-json/artifacts/research-sources.json");
    assert_eq!(resolved.map(|value| value.path), Some(expected.clone()));
    let written = std::fs::read_to_string(expected).expect("read recovered artifact");
    let parsed: serde_json::Value = serde_json::from_str(&written).expect("parse recovered json");
    assert_eq!(parsed["sources"][0]["path"], "README.md");
    assert_eq!(parsed["summary"], "Primary local sources identified.");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_unwraps_json_handoff_wrapper_from_session_text() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-session-text-json-wrapper-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-session-json-wrapper";
    let output_path = ".tandem/artifacts/research-sources.json";
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut session = Session::new(Some("session text wrapper recovery".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::Text {
            text: "{\n  \"structured_handoff\": {\n    \"sources\": [\n      {\n        \"path\": \"README.md\",\n        \"reason\": \"project overview\"\n      }\n    ],\n    \"summary\": \"Primary local sources identified.\"\n  }\n}\n{\"status\":\"completed\"}".to_string(),
        }],
    ));

    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "research_sources".to_string(),
            agent_id: "researcher".to_string(),
            objective: "Find and record local sources".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        50,
        10,
    )
    .await
    .expect("recover wrapped session text");

    let expected = workspace_root
        .join(".tandem/runs/run-session-json-wrapper/artifacts/research-sources.json");
    assert_eq!(resolved.map(|value| value.path), Some(expected.clone()));
    let written = std::fs::read_to_string(expected).expect("read recovered artifact");
    let parsed: serde_json::Value = serde_json::from_str(&written).expect("parse recovered json");
    assert_eq!(parsed["sources"][0]["path"], "README.md");
    assert_eq!(parsed["summary"], "Primary local sources identified.");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_promotes_legacy_workspace_artifact_into_run_scope() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-legacy-promotion-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-legacy-promotion";
    let output_path = ".tandem/artifacts/research-sources.json";
    let legacy_path = workspace_root.join(output_path);
    std::fs::create_dir_all(legacy_path.parent().expect("legacy parent"))
        .expect("create legacy parent");
    std::fs::write(&legacy_path, "{\n  \"status\": \"completed\"\n}")
        .expect("write legacy artifact");

    let mut session = Session::new(Some("legacy promotion".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": output_path,
                "content": "{\n  \"status\": \"completed\"\n}"
            }),
            result: Some(json!({"output": "written"})),
            error: None,
        }],
    ));

    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "research_sources".to_string(),
            agent_id: "researcher".to_string(),
            objective: "Find and record sources".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        50,
        10,
    )
    .await
    .expect("promote legacy artifact")
    .expect("resolution");

    let expected =
        workspace_root.join(".tandem/runs/run-legacy-promotion/artifacts/research-sources.json");
    assert_eq!(resolved.path, expected);
    assert_eq!(
        resolved.legacy_workspace_artifact_promoted_from,
        Some(legacy_path.clone())
    );
    let promoted = std::fs::read_to_string(&resolved.path).expect("read promoted artifact");
    assert!(promoted.contains("\"status\": \"completed\""));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_does_not_promote_unrelated_workspace_file() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-no-unrelated-promotion-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-no-promotion";
    let output_path = ".tandem/artifacts/research-sources.json";
    let unrelated_path = workspace_root.join(".tandem/knowledge/research-sources.json");
    std::fs::create_dir_all(unrelated_path.parent().expect("unrelated parent"))
        .expect("create unrelated parent");
    std::fs::write(&unrelated_path, "{\n  \"status\": \"completed\"\n}")
        .expect("write unrelated file");

    let mut session = Session::new(Some("unrelated write".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": ".tandem/knowledge/research-sources.json",
                "content": "{\n  \"status\": \"completed\"\n}"
            }),
            result: Some(json!({"output": "written"})),
            error: None,
        }],
    ));

    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "research_sources".to_string(),
            agent_id: "researcher".to_string(),
            objective: "Find and record sources".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        50,
        10,
    )
    .await
    .expect("resolve unrelated file");

    assert!(resolved.is_none());
    assert!(!workspace_root
        .join(".tandem/runs/run-no-promotion/artifacts/research-sources.json")
        .exists());

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_verified_output_snapshot_replace_copies_into_workspace_target() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-publish-workspace-{}", uuid::Uuid::new_v4()));
    let run_artifact = workspace_root.join(".tandem/runs/run-publish/artifacts/report.md");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "# Report\n").expect("write run artifact");

    let automation = AutomationV2Spec {
        automation_id: "automation-publish".to_string(),
        name: "Publish".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: Default::default(),
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
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let mut node = bare_node();
    node.node_id = "generate_report".to_string();

    let result = super::publish_automation_verified_output(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-publish",
        &node,
        &(
            ".tandem/runs/run-publish/artifacts/report.md".to_string(),
            "# Report\n".to_string(),
        ),
        &super::AutomationArtifactPublishSpec {
            scope: super::AutomationArtifactPublishScope::Workspace,
            path: ".tandem/knowledge/report-latest.md".to_string(),
            mode: super::AutomationArtifactPublishMode::SnapshotReplace,
        },
    )
    .expect("publish to workspace");

    let published = workspace_root.join(".tandem/knowledge/report-latest.md");
    assert_eq!(
        std::fs::read_to_string(&published).expect("read published"),
        "# Report\n"
    );
    assert_eq!(result["scope"], "workspace");
    assert_eq!(result["mode"], "snapshot_replace");
    assert_eq!(result["path"], ".tandem/knowledge/report-latest.md");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_verified_output_snapshot_replace_copies_into_global_target() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-publish-global-workspace-{}",
        uuid::Uuid::new_v4()
    ));
    let run_artifact = workspace_root.join(".tandem/runs/run-publish-global/artifacts/report.md");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "# Global Report\n").expect("write run artifact");

    let automation = AutomationV2Spec {
        automation_id: "automation-global-publish".to_string(),
        name: "Publish Global".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: Default::default(),
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
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let mut node = bare_node();
    node.node_id = "generate_report".to_string();
    let relative_global_path = format!("test-{}/report.md", uuid::Uuid::new_v4());

    let result = super::publish_automation_verified_output(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-publish-global",
        &node,
        &(
            ".tandem/runs/run-publish-global/artifacts/report.md".to_string(),
            "# Global Report\n".to_string(),
        ),
        &super::AutomationArtifactPublishSpec {
            scope: super::AutomationArtifactPublishScope::Global,
            path: relative_global_path.clone(),
            mode: super::AutomationArtifactPublishMode::SnapshotReplace,
        },
    )
    .expect("publish to global");

    let published_root = crate::config::paths::resolve_automation_published_artifacts_dir();
    let published = published_root.join(&relative_global_path);
    assert_eq!(
        std::fs::read_to_string(&published).expect("read published"),
        "# Global Report\n"
    );
    assert_eq!(result["scope"], "global");
    assert_eq!(result["mode"], "snapshot_replace");
    assert_eq!(
        result["path"],
        json!(published.to_string_lossy().to_string())
    );

    let _ = std::fs::remove_file(&published);
    if let Some(parent) = published.parent() {
        let _ = std::fs::remove_dir(parent);
    }
    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_verified_output_append_jsonl_appends_records() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-publish-append-jsonl-{}",
        uuid::Uuid::new_v4()
    ));
    let run_artifact = workspace_root.join(".tandem/runs/run-append/artifacts/research.json");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "{\n  \"sources\": [\"README.md\"]\n}")
        .expect("write run artifact");

    let automation = AutomationV2Spec {
        automation_id: "automation-append".to_string(),
        name: "Append".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: Default::default(),
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
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let mut node = bare_node();
    node.node_id = "research_sources".to_string();
    let publish_path = ".tandem/knowledge/research-history.jsonl";

    super::publish_automation_verified_output(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-append",
        &node,
        &(
            ".tandem/runs/run-append/artifacts/research.json".to_string(),
            "{\n  \"sources\": [\"README.md\"]\n}".to_string(),
        ),
        &super::AutomationArtifactPublishSpec {
            scope: super::AutomationArtifactPublishScope::Workspace,
            path: publish_path.to_string(),
            mode: super::AutomationArtifactPublishMode::AppendJsonl,
        },
    )
    .expect("first append");
    super::publish_automation_verified_output(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-append-2",
        &node,
        &(
            ".tandem/runs/run-append/artifacts/research.json".to_string(),
            "{\n  \"sources\": [\"README.md\"]\n}".to_string(),
        ),
        &super::AutomationArtifactPublishSpec {
            scope: super::AutomationArtifactPublishScope::Workspace,
            path: publish_path.to_string(),
            mode: super::AutomationArtifactPublishMode::AppendJsonl,
        },
    )
    .expect("second append");

    let published = workspace_root.join(publish_path);
    let lines = std::fs::read_to_string(&published)
        .expect("read appended file")
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    let first: Value = serde_json::from_str(&lines[0]).expect("parse first");
    let second: Value = serde_json::from_str(&lines[1]).expect("parse second");
    assert_eq!(first["run_id"], "run-append");
    assert_eq!(second["run_id"], "run-append-2");
    assert_eq!(first["content"]["sources"][0], "README.md");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_verified_output_falls_back_to_automation_output_targets() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-publish-targets-{}", uuid::Uuid::new_v4()));
    let run_artifact = workspace_root.join(".tandem/runs/run-targets/artifacts/report.md");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "# Targeted Report\n").expect("write run artifact");

    let automation = AutomationV2Spec {
        automation_id: "automation-targets".to_string(),
        name: "Targets".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: Default::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec!["notes/final-report.md".to_string()],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = bare_node();

    let result = super::publish_automation_verified_outputs(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-targets",
        &node,
        &(
            ".tandem/runs/run-targets/artifacts/report.md".to_string(),
            "# Targeted Report\n".to_string(),
        ),
    )
    .expect("publish to output targets");

    let published = workspace_root.join("notes/final-report.md");
    assert_eq!(
        std::fs::read_to_string(&published).expect("read published"),
        "# Targeted Report\n"
    );
    assert_eq!(result["targets"][0]["scope"], "workspace");
    assert_eq!(result["targets"][0]["mode"], "snapshot_replace");
    assert_eq!(result["targets"][0]["path"], "notes/final-report.md");
    assert_eq!(result["targets"][0]["copied"], true);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_verified_output_rejects_workspace_target_outside_workspace() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-publish-invalid-workspace-{}",
        uuid::Uuid::new_v4()
    ));
    let run_artifact = workspace_root.join(".tandem/runs/run-invalid/artifacts/report.md");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "# Report\n").expect("write run artifact");

    let automation = AutomationV2Spec {
        automation_id: "automation-invalid-publish".to_string(),
        name: "Invalid Publish".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: Default::default(),
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
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = bare_node();

    let error = super::publish_automation_verified_output(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-invalid",
        &node,
        &(
            ".tandem/runs/run-invalid/artifacts/report.md".to_string(),
            "# Report\n".to_string(),
        ),
        &super::AutomationArtifactPublishSpec {
            scope: super::AutomationArtifactPublishScope::Workspace,
            path: "../outside/report.md".to_string(),
            mode: super::AutomationArtifactPublishMode::SnapshotReplace,
        },
    )
    .expect_err("workspace publish should fail");

    assert!(error.to_string().contains("must stay inside workspace"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn session_write_candidates_accepts_file_path_schema_with_normalized_run_scoped_paths() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-write-candidate-file-path-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let run_id = "run-123";
    let artifact_path_with_dot_segments = workspace_root
        .join(".tandem/runs/run-123/artifacts")
        .join("./report.md");

    let mut session = Session::new(Some("file path candidate".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "filePath": artifact_path_with_dot_segments.to_string_lossy(),
                "body": "report body"
            }),
            result: Some(json!({"output":"written"})),
            error: None,
        }],
    ));

    let candidates = session_write_candidates_for_output(
        &session,
        workspace_root.to_str().expect("workspace root"),
        ".tandem/artifacts/report.md",
        Some(run_id),
        None,
    );

    assert_eq!(candidates, vec!["report body".to_string()]);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn session_write_materialized_output_accepts_absolute_legacy_artifact_paths() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-current-attempt-output-abs-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-abs";
    let legacy_abs_path = workspace_root
        .join(".tandem/artifacts/report.md")
        .to_string_lossy()
        .to_string();
    let run_scoped_path = workspace_root.join(".tandem/runs/run-abs/artifacts/report.md");
    std::fs::create_dir_all(
        run_scoped_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create run artifacts dir");
    std::fs::write(&run_scoped_path, "report body").expect("write run-scoped artifact");

    let mut session = Session::new(Some("absolute write evidence".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": legacy_abs_path,
                "content": "report body"
            }),
            result: Some(json!({"output":"ok"})),
            error: None,
        }],
    ));

    assert!(session_write_materialized_output_for_output(
        &session,
        workspace_root.to_str().expect("workspace root"),
        ".tandem/artifacts/report.md",
        Some(run_id),
        None,
    ));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn session_write_materialized_output_accepts_file_path_schema_with_normalized_run_scoped_paths() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-current-attempt-output-file-path-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-file-path";
    let artifact_path = workspace_root.join(".tandem/runs/run-file-path/artifacts/report.md");
    let artifact_path_with_dot_segments = workspace_root
        .join(".tandem/runs/run-file-path/artifacts")
        .join("./report.md");
    std::fs::create_dir_all(
        artifact_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create artifacts dir");
    std::fs::write(&artifact_path, "report body").expect("write artifact");

    let mut session = Session::new(Some("file path write evidence".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "filePath": artifact_path_with_dot_segments.to_string_lossy(),
                "content": "report body"
            }),
            result: Some(json!({"output":"written"})),
            error: None,
        }],
    ));

    assert!(session_write_materialized_output_for_output(
        &session,
        workspace_root.to_str().expect("workspace root"),
        ".tandem/artifacts/report.md",
        Some(run_id),
        None,
    ));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn session_write_candidates_supports_variant_path_and_content_keys() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-write-candidate-variants-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut session = Session::new(Some("candidate variants".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "output_path": ".tandem/artifacts/report.md",
                "contents": "variant payload"
            }),
            result: Some(json!({"output":"ok"})),
            error: None,
        }],
    ));

    let candidates = session_write_candidates_for_output(
        &session,
        workspace_root.to_str().expect("workspace root"),
        ".tandem/artifacts/report.md",
        Some("run-variants"),
        None,
    );
    assert_eq!(candidates, vec!["variant payload".to_string()]);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn resolve_automation_output_path_rejects_parent_escape_segments() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-output-path-escape-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let resolved = resolve_automation_output_path(
        workspace_root.to_str().expect("workspace root"),
        "../outside.md",
    );
    assert!(
        resolved.is_err(),
        "expected parent escape path to be rejected, got {resolved:?}"
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn resolve_automation_output_path_normalizes_dot_segments_inside_workspace() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-output-path-normalize-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("nested")).expect("create workspace");

    let resolved = resolve_automation_output_path(
        workspace_root.to_str().expect("workspace root"),
        "nested/../report.md",
    )
    .expect("resolve normalized path");

    assert_eq!(resolved, workspace_root.join("report.md"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

// -----------------------------------------------------------------------
// assess_artifact_candidate — score ordering invariants
// -----------------------------------------------------------------------

#[test]
fn assess_empty_text_has_negative_score() {
    let assessment =
        assess_artifact_candidate(&bare_node(), "/workspace", "tool", "", &[], &[], &[], &[]);
    assert!(
        assessment.score < 0,
        "empty text should produce a negative score, got {}",
        assessment.score
    );
}

#[test]
fn assess_substantive_text_scores_higher_than_empty() {
    let rich = "## Summary\n\nDetailed analysis.\n\n## Files reviewed\n\n- /workspace/foo.rs\n\n## Approved\n\nYes.";
    let rich_score =
        assess_artifact_candidate(&bare_node(), "/workspace", "tool", rich, &[], &[], &[], &[])
            .score;
    let empty_score =
        assess_artifact_candidate(&bare_node(), "/workspace", "tool", "", &[], &[], &[], &[]).score;
    assert!(
        rich_score > empty_score,
        "substantive text ({rich_score}) should score higher than empty ({empty_score})"
    );
}

#[test]
fn assess_source_field_preserved() {
    let assessment = assess_artifact_candidate(
        &bare_node(),
        "/workspace",
        "my_source",
        "hello",
        &[],
        &[],
        &[],
        &[],
    );
    assert_eq!(assessment.source, "my_source");
}

#[test]
fn assess_evidence_anchors_count_upstream_path_and_url_mentions() {
    let assessment = assess_artifact_candidate(
        &bare_node(),
        "/workspace",
        "tool",
        "See /workspace/docs/product-capabilities.md and https://example.com/source-1 for details.",
        &[],
        &[],
        &[
            "/workspace/docs/product-capabilities.md".to_string(),
            "/workspace/README.md".to_string(),
        ],
        &["https://example.com/source-1".to_string()],
    );
    assert!(
        assessment.evidence_anchor_count >= 2,
        "expected to match at least two upstream evidence anchors, got {}",
        assessment.evidence_anchor_count
    );
}

// -----------------------------------------------------------------------
// Standup gap fill — T1: filler detection consolidation (item E)
// -----------------------------------------------------------------------

// Converts raw standup JSON into the upstream input shape that
// extract_standup_participant_update() and the filler detectors consume.
fn standup_participant_input(node_id: &str, yesterday: &str, today: &str) -> Value {
    json!({
        "alias": node_id,
        "from_step_id": node_id,
        "output": {
            "status": "completed",
            "content": {
                "text": serde_json::to_string(&json!({
                    "yesterday": yesterday,
                    "today": today,
                    "status": "completed"
                })).unwrap()
            }
        }
    })
}

#[test]
fn standup_filler_detection_catches_standup_specific_phrases() {
    use super::node_output::detect_automation_node_status;
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "standup_update".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    // Both fields contain standup-specific filler phrases
    let session_text = serde_json::to_string(&json!({
        "yesterday": "reviewed workspace artifacts and tandem memory; identified relevant context",
        "today": "prepare the daily standup report from available context",
        "status": "completed"
    }))
    .unwrap();
    let (status, reason, _) =
        detect_automation_node_status(&node, &session_text, None, &json!({}), None);
    assert_eq!(
        status, "needs_repair",
        "standup-specific filler phrases should trigger needs_repair"
    );
    assert!(
        reason.is_some(),
        "filler rejection should include a repair reason"
    );
}

#[test]
fn standup_filler_detection_catches_generic_placeholder_phrases() {
    use super::node_output::detect_automation_node_status;
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "standup_update".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    // Generic status-only markers that placeholder_like_artifact_text() catches:
    // short text containing "completed", "confirmed", "write completion", etc.
    // These represent agents that respond with status echo strings instead of content.
    let session_text = serde_json::to_string(&json!({
        "yesterday": "completed",
        "today": "write completion",
        "status": "completed"
    }))
    .unwrap();
    let (status, _reason, _) =
        detect_automation_node_status(&node, &session_text, None, &json!({}), None);
    assert_eq!(
        status, "needs_repair",
        "generic placeholder phrases should also trigger needs_repair via consolidated detection"
    );
}

#[test]
fn standup_filler_detection_accepts_concrete_updates() {
    use super::node_output::detect_automation_node_status;
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "standup_update".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    // Concrete update with real file references
    let session_text = serde_json::to_string(&json!({
        "yesterday": "Drafted homepage headline copy in outputs/homepage-copy.md and refined the H1 variant list.",
        "today": "Update the campaign brief with the new audience segment based on outputs/research-brief.md.",
        "status": "completed"
    }))
    .unwrap();
    let (status, _reason, _) =
        detect_automation_node_status(&node, &session_text, None, &json!({}), None);
    assert_eq!(
        status, "completed",
        "concrete standup update with file references should be accepted"
    );
}

// -----------------------------------------------------------------------
// Standup gap fill — T2: enriched repair reason (item D)
// -----------------------------------------------------------------------

#[test]
fn standup_filler_repair_reason_includes_tool_telemetry_context() {
    use super::node_output::detect_automation_node_status;
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "standup_update".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    let session_text = serde_json::to_string(&json!({
        "yesterday": "reviewed workspace artifacts and tandem memory",
        "today": "prepare the daily standup report from available context",
        "status": "completed"
    }))
    .unwrap();
    let tool_telemetry = json!({
        "executed_tools": ["glob", "read", "memory_search"],
        "glob_directories": ["outputs/", "content/"],
        "read_paths": ["outputs/homepage-copy.md", "content/article-draft.md"]
    });
    let (status, reason, _) =
        detect_automation_node_status(&node, &session_text, None, &tool_telemetry, None);
    assert_eq!(status, "needs_repair");
    let reason = reason.expect("filler rejection should include a reason");
    assert!(
        reason.contains("glob") || reason.contains("read"),
        "repair reason should mention tools used, got: {reason}"
    );
    assert!(
        reason.contains("outputs/") || reason.contains("content/"),
        "repair reason should mention directories searched, got: {reason}"
    );
    assert!(
        reason.contains("homepage-copy") || reason.contains("article-draft"),
        "repair reason should mention files read, got: {reason}"
    );
}

#[test]
fn standup_filler_repair_reason_handles_missing_telemetry_gracefully() {
    use super::node_output::detect_automation_node_status;
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "standup_update".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    let session_text = serde_json::to_string(&json!({
        "yesterday": "reviewed workspace",
        "today": "workspace context",
        "status": "completed"
    }))
    .unwrap();
    let (status, reason, _) =
        detect_automation_node_status(&node, &session_text, None, &json!({}), None);
    assert_eq!(status, "needs_repair");
    let reason = reason.expect("filler rejection should always include a reason");
    assert!(
        reason.contains("none recorded"),
        "missing telemetry should not cause panic; got: {reason}"
    );
}

// -----------------------------------------------------------------------
// Standup gap fill — T3: receipt path derivation (item B)
// -----------------------------------------------------------------------

#[test]
fn standup_receipt_path_derived_from_report_path() {
    // Test the standup_receipt_path_for_report helper directly
    // The function is private, so we test it indirectly through compile-time
    // inclusion. We verify the expected pattern holds for our documented example.
    let report = "docs/standups/2026-04-05.md";
    let receipt = super::standup_receipt_path_for_report(report);
    assert_eq!(receipt, "docs/standups/receipt-2026-04-05.json");
}

#[test]
fn standup_receipt_path_handles_root_level_report() {
    let report = "standup.md";
    let receipt = super::standup_receipt_path_for_report(report);
    assert_eq!(receipt, "docs/standups/receipt-standup.json");
}

#[test]
fn standup_receipt_path_handles_nested_report() {
    let report = "team/standups/weekly/2026-04-05.md";
    let receipt = super::standup_receipt_path_for_report(report);
    assert_eq!(receipt, "team/standups/weekly/receipt-2026-04-05.json");
}

#[test]
fn standup_synthesis_effective_required_output_path_uses_report_template() {
    let automation = AutomationV2Spec {
        automation_id: "automation-standup".to_string(),
        name: "Daily Standup".to_string(),
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
        output_targets: vec!["docs/standups/{{date}}.md".to_string()],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: Some(json!({
            "feature": "agent_standup",
            "standup": {
                "report_path_template": "docs/standups/{{date}}.md"
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "standup_synthesis".to_string(),
        agent_id: "coordinator".to_string(),
        objective: "Write the standup report".to_string(),
        depends_on: vec!["participant_0".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "participant_0".to_string(),
            alias: "participant_0".to_string(),
        }],
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
        stage_kind: Some(crate::AutomationNodeStageKind::Orchestrator),
        gate: None,
        metadata: None,
    };
    let started_at_ms = chrono::DateTime::parse_from_rfc3339("2026-04-14T09:00:00Z")
        .expect("timestamp")
        .timestamp_millis() as u64;

    let output_path = super::automation_effective_required_output_path_for_run(
        &automation,
        &node,
        "automation-v2-run-standup",
        started_at_ms,
    );

    assert_eq!(output_path.as_deref(), Some("docs/standups/2026-04-14.md"));
}

#[test]
fn parse_status_json_accepts_standup_completion_metadata() {
    let raw = "Standup report written to `docs/standups/2026-04-14.md` for 3 participants.\n\n{\"status\":\"completed\",\"approved\":true,\"report_path\":\"docs/standups/2026-04-14.md\",\"participant_count\":3}";

    let parsed = super::parse_status_json(raw).expect("standup status payload should parse");

    assert_eq!(
        parsed.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        parsed.get("report_path").and_then(Value::as_str),
        Some("docs/standups/2026-04-14.md")
    );
    assert_eq!(
        parsed.get("participant_count").and_then(Value::as_u64),
        Some(3)
    );
}

// -----------------------------------------------------------------------
// Standup gap fill — T5: coordinator input formatting (item C)
// -----------------------------------------------------------------------

#[test]
fn extract_standup_participant_update_finds_nested_json_in_content_text() {
    let input = standup_participant_input(
        "participant_0_copywriter",
        "Drafted homepage headline copy in outputs/homepage-copy.md",
        "Refine the H1 variants based on the new positioning brief",
    );
    let update = super::prompting_impl::extract_standup_participant_update_pub(&input);
    assert!(
        update.is_some(),
        "should extract standup update from content.text JSON"
    );
    let update = update.unwrap();
    assert!(
        update.get("yesterday").is_some(),
        "extracted update should have yesterday field"
    );
    assert!(
        update.get("today").is_some(),
        "extracted update should have today field"
    );
}

#[test]
fn extract_standup_participant_update_returns_none_for_non_standup_output() {
    let input = json!({
        "alias": "research_brief",
        "from_step_id": "research_brief",
        "output": {
            "status": "completed",
            "content": {
                "text": "The research findings indicate three key market opportunities..."
            }
        }
    });
    let update = super::prompting_impl::extract_standup_participant_update_pub(&input);
    assert!(
        update.is_none(),
        "non-standup output text should not be mistaken for a participant update"
    );
}
