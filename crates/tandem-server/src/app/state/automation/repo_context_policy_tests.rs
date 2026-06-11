use super::*;

fn code_workflow_node() -> AutomationFlowNode {
    AutomationFlowNode {
        node_id: "code_task".to_string(),
        agent_id: "agent".to_string(),
        objective: "Patch the repository bug.".to_string(),
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": { "task_kind": "code_change" }
        })),
    }
}

#[test]
fn code_workflow_defaults_include_repo_context_tools() {
    let node = code_workflow_node();

    let requested = normalize_automation_requested_tools(&node, "/home/evan/tandem", Vec::new());

    assert!(requested.contains(&"repo.context_bundle".to_string()));
    assert!(requested.contains(&"repo.search".to_string()));
    assert!(requested.contains(&"repo.symbol".to_string()));
    assert!(requested.contains(&"glob".to_string()));
    assert!(requested.contains(&"read".to_string()));
    assert!(requested.contains(&"edit".to_string()));
    assert!(requested.contains(&"apply_patch".to_string()));
    assert!(requested.contains(&"write".to_string()));
    assert!(requested.contains(&"bash".to_string()));
}
