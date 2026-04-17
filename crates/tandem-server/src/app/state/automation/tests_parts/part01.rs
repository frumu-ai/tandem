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

