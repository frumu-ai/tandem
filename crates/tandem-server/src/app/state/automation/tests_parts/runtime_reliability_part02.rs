// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[test]
fn automation_prompt_includes_sanitized_webhook_trigger_context() {
    let mut automation = automation_with_output_targets(Vec::new(), Vec::new());
    automation.metadata = Some(json!({
        "automation_webhook": {
            "provider": "generic",
            "provider_event_kind": "customer.incident_reported",
            "provider_event_id": "incident-demo-1",
            "trust": "untrusted_external_webhook",
            "preview": {
                "event": "customer.incident_reported",
                "event_id": "incident-demo-1",
                "customer_name": "ACME",
                "service": "Payments API",
                "summary": "Elevated 5xx errors affecting checkout"
            }
        }
    }));
    let node = bare_node();
    let agent = AutomationAgentProfile {
        agent_id: "agent-demo".to_string(),
        template_id: None,
        display_name: "Demo Agent".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: AutomationAgentToolPolicy {
            allowlist: Vec::new(),
            denylist: Vec::new(),
        },
        mcp_policy: AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp/workspace",
        "run-demo",
        &node,
        1,
        &agent,
        &[],
        &[],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Run Trigger Context:"));
    assert!(prompt.contains("\"customer_name\": \"ACME\""));
    assert!(prompt.contains("\"service\": \"Payments API\""));
    assert!(prompt.contains("untrusted external data"));
}

#[test]
fn approval_gates_are_not_outbound_actions() {
    let mut node = bare_node();
    node.objective = "Approve this draft before publishing it.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "approval_gate".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });

    assert!(!automation_node_is_outbound_action(&node));
}

#[test]
fn gated_local_publications_are_not_outbound_actions() {
    let mut node = bare_node();
    node.node_id = "publish_approved_update".to_string();
    node.objective = "Publish the approved update as a local artifact.".to_string();
    node.metadata = Some(json!({
        "artifact": {
            "path": "demo-output/approved/incident-update.md",
            "visible_in_run": true
        },
        "builder": {
            "role": "publisher",
            "task_kind": "delivery",
            "task_class": "gated_local_publish"
        }
    }));

    assert!(!automation_node_is_outbound_action(&node));
}

#[test]
fn connector_triage_nodes_are_not_outbound_actions() {
    let mut node = bare_node();
    node.node_id = "assess_issue".to_string();
    node.objective = "Classify the issue initial urgency and delivery/security risk.".to_string();
    node.metadata = Some(json!({
        "triage_gate": true,
        "builder": {
            "task_kind": "delivery",
            "task_class": "connector_triage",
            "triage_model": true
        }
    }));

    assert!(!automation_node_is_outbound_action(&node));
}

#[test]
fn delivery_risk_language_is_not_an_outbound_action() {
    let mut node = bare_node();
    node.objective = "Assess delivery/security/operational risk before planning.".to_string();

    assert!(!automation_node_is_outbound_action(&node));
}

#[test]
fn explicit_deliver_action_remains_outbound() {
    let mut node = bare_node();
    node.objective = "Deliver the approved report to the customer.".to_string();

    assert!(automation_node_is_outbound_action(&node));
}

#[test]
fn deliver_action_with_punctuation_remains_outbound() {
    for objective in ["Deliver.", "Deliver: final report", "Prepare the report, then deliver"] {
        let mut node = bare_node();
        node.objective = objective.to_string();
        assert!(automation_node_is_outbound_action(&node), "{objective}");
    }
}

#[test]
fn planner_delivery_publication_gets_a_run_artifact_path() {
    let mut node = bare_node();
    node.node_id = "publish_approved_update".to_string();
    node.objective = "Publish the approved local artifact.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "artifact": {
            "path": "demo-output/approved/incident-update.md",
            "visible_in_run": true
        },
        "builder": {
            "task_kind": "delivery",
            "task_class": "gated_local_publish"
        }
    }));

    assert_eq!(
        super::node_runtime_impl::automation_node_default_output_path(&node).as_deref(),
        Some(".tandem/artifacts/publish-approved-update.md")
    );
    assert_eq!(
        super::automation_node_required_output_path_for_run(&node, Some("run-approved"))
            .as_deref(),
        Some(".tandem/runs/run-approved/artifacts/publish-approved-update.md")
    );
}

#[test]
fn capability_ids_exact_path_read_write_node_excludes_workspace_discover() {
    let mut node = bare_node();
    node.node_id = "draft_incident_update".to_string();
    node.objective = "Use read on demo-output/context/incident-context.md, then write demo-output/drafts/incident-update.md.".to_string();
    node.metadata = Some(json!({
        "artifact": {
            "path": "demo-output/drafts/incident-update.md",
            "visible_in_run": true
        },
        "filesystem_policy": {
            "read_paths": ["demo-output/context/incident-context.md"],
            "write_paths": ["demo-output/drafts/incident-update.md"]
        },
        "required_tools": ["read", "write"],
        "tool_allowlist": ["read", "write"]
    }));

    let caps = automation_tool_capability_ids(&node, "artifact_write");

    assert!(caps.contains(&"workspace_read".to_string()));
    assert!(caps.contains(&"artifact_write".to_string()));
    assert!(
        !caps.contains(&"workspace_discover".to_string()),
        "an exact-path read/write node must not require an unrequested discovery tool: {caps:?}"
    );
}

#[test]
fn capability_ids_human_approval_excludes_artifact_write() {
    let mut node = bare_node();
    node.node_id = "human_approval".to_string();
    node.objective = "Pause for approval, use no tools, and do not create, modify, or delete files.".to_string();
    node.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "draft_incident_update".to_string(),
        alias: "draft_for_review".to_string(),
    }];
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "approval_gate".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "stage_kind": "Approval",
        "approval": {
            "allowed_decisions": ["Approve", "Reject"],
            "require_explicit_decision": true
        },
        "builder": {
            "task_class": "human_decision_gate"
        },
        "tool_allowlist": []
    }));

    let caps = automation_tool_capability_ids(&node, "artifact_write");

    assert!(
        !caps.contains(&"artifact_write".to_string()),
        "human approval must not infer a write capability from negated file language: {caps:?}"
    );
}
