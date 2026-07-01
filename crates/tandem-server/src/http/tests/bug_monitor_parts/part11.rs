#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_authority_inventory_returns_empty_sections() {
    let state = test_state().await;
    let app = app_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bug-monitor/security/authority-inventory")
                .body(Body::empty())
                .expect("authority inventory request"),
        )
        .await
        .expect("authority inventory response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("authority inventory body");
    let payload: Value = serde_json::from_slice(&body).expect("authority inventory json");
    assert_eq!(payload["schema_version"], json!(1));
    assert_eq!(payload["scope"]["read_only"], json!(true));
    assert!(payload["inventory"]["workflows"].as_array().is_some());
    assert!(payload["inventory"]["automation_specs"]
        .as_array()
        .is_some());
    assert!(payload["inventory"]["scoped_intake_keys"]
        .as_array()
        .expect("intake keys array")
        .is_empty());
    assert!(payload["inventory"]["mcp"]["servers"].as_array().is_some());
    assert_eq!(payload["counts"]["automation_specs"], json!(0));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_authority_inventory_summarizes_authority_and_redacts_secrets() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("authority inventory workspace");
    std::fs::create_dir_all(workspace.path().join("logs")).expect("logs dir");

    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(workspace.path().display().to_string()),
            destinations: vec![
                crate::BugMonitorDestinationConfig {
                    destination_id: "linear-prod".to_string(),
                    name: "Linear production".to_string(),
                    kind: crate::BugMonitorDestinationKind::LinearIssue,
                    enabled: true,
                    require_approval: true,
                    linear_team: Some("eng".to_string()),
                    linear_project: Some("incident-monitor".to_string()),
                    ..Default::default()
                },
                crate::BugMonitorDestinationConfig {
                    destination_id: "signed-webhook".to_string(),
                    name: "Signed webhook".to_string(),
                    kind: crate::BugMonitorDestinationKind::Webhook,
                    enabled: true,
                    webhook_url: Some("https://hooks.example.test/incidents".to_string()),
                    webhook_secret_ref: Some("env:BUG_MONITOR_AUTHORITY_SECRET".to_string()),
                    config: Some(json!({
                        "headers": { "authorization": "Bearer must-not-leak" },
                        "template": "redacted-by-inventory"
                    })),
                    ..Default::default()
                },
                crate::BugMonitorDestinationConfig {
                    destination_id: "mcp-tool".to_string(),
                    name: "MCP tool".to_string(),
                    kind: crate::BugMonitorDestinationKind::McpTool,
                    enabled: true,
                    mcp_server: Some("github".to_string()),
                    mcp_tool: Some("mcp.github.create_pull_request".to_string()),
                    config: Some(json!({
                        "allow_publish": true,
                        "payload_mapping": { "title": "$draft.title" }
                    })),
                    ..Default::default()
                },
            ],
            routes: vec![crate::BugMonitorRouteConfig {
                route_id: "ci-linear".to_string(),
                name: "CI incidents to Linear".to_string(),
                priority: 10,
                destination_ids: vec!["linear-prod".to_string()],
                approval_policy: crate::BugMonitorApprovalPolicy::Always,
                match_source_kinds: vec!["ci".to_string()],
                match_project_ids: vec!["payments".to_string()],
                match_tenant_ids: vec!["tenant-a".to_string()],
                match_workspace_ids: vec!["workspace-a".to_string()],
                ..Default::default()
            }],
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/payments".to_string(),
                workspace_root: workspace.path().display().to_string(),
                source_kind: crate::BugMonitorSourceKind::ExternalApp,
                allowed_destination_ids: vec!["linear-prod".to_string(), "mcp-tool".to_string()],
                default_destination_ids: vec!["linear-prod".to_string()],
                tenant_id: Some("tenant-a".to_string()),
                workspace_id: Some("workspace-a".to_string()),
                approval_policy: crate::BugMonitorApprovalPolicy::HighRisk,
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    source_kind: Some(crate::BugMonitorSourceKind::Ci),
                    allowed_destination_ids: vec!["linear-prod".to_string()],
                    default_destination_ids: vec!["linear-prod".to_string()],
                    default_route_tags: vec!["prod".to_string()],
                    tenant_id: Some("tenant-a".to_string()),
                    workspace_id: Some("workspace-a".to_string()),
                    approval_policy: crate::BugMonitorApprovalPolicy::Always,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            default_destination_ids: vec!["linear-prod".to_string()],
            ..Default::default()
        })
        .await
        .expect("config");

    let raw_key = "tbm_intake_authority_inventory_secret";
    let key_hash = crate::sha256_hex(&[raw_key]);
    state
        .put_bug_monitor_intake_key(crate::BugMonitorProjectIntakeKey {
            key_id: "intake-key-authority".to_string(),
            project_id: "payments".to_string(),
            name: "CI report key".to_string(),
            key_hash: key_hash.clone(),
            enabled: true,
            scopes: vec!["bug_monitor:report".to_string()],
            created_at_ms: Some(crate::now_ms()),
            last_used_at_ms: None,
        })
        .await
        .expect("intake key");

    let automation = sample_authority_inventory_automation(workspace.path().display().to_string());
    state
        .put_automation_v2(automation)
        .await
        .expect("automation");
    state
        .record_external_action(crate::ExternalActionRecord {
            action_id: "action-authority-1".to_string(),
            operation: "create_linear_issue".to_string(),
            status: "posted".to_string(),
            source_kind: Some("incident_monitor".to_string()),
            source_id: Some("draft-1".to_string()),
            provider: Some("linear".to_string()),
            target: Some("linear-prod".to_string()),
            approval_state: Some("approved".to_string()),
            receipt: Some(json!({ "secret": "receipt-must-not-leak" })),
            metadata: Some(json!({ "private_note": "metadata-must-not-leak" })),
            created_at_ms: 10,
            updated_at_ms: 20,
            ..Default::default()
        })
        .await
        .expect("external action");

    let app = app_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bug-monitor/security/authority-inventory")
                .body(Body::empty())
                .expect("authority inventory request"),
        )
        .await
        .expect("authority inventory response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("authority inventory body");
    let payload: Value = serde_json::from_slice(&body).expect("authority inventory json");

    let automations = payload["inventory"]["automation_specs"]
        .as_array()
        .expect("automation specs");
    let automation = automations
        .iter()
        .find(|row| row["automation_id"].as_str() == Some("auto-authority-inventory"))
        .expect("authority automation");
    assert!(automation["agents"][0]["tool_policy"]["allowlist"]
        .as_array()
        .expect("agent allowlist")
        .iter()
        .any(|tool| tool.as_str() == Some("write")));
    assert!(automation["agents"][0]["mcp_policy"]["allowed_tools"]
        .as_array()
        .expect("agent mcp tools")
        .iter()
        .any(|tool| tool.as_str() == Some("mcp.github.create_pull_request")));

    let mcp_servers = payload["inventory"]["mcp"]["servers"]
        .as_array()
        .expect("mcp servers");
    assert!(mcp_servers
        .iter()
        .any(|server| server["name"].as_str() == Some("github")));

    let destinations = payload["inventory"]["destinations"]
        .as_array()
        .expect("destinations");
    let linear = destinations
        .iter()
        .find(|row| row["destination_id"].as_str() == Some("linear-prod"))
        .expect("linear destination");
    assert_eq!(linear["require_approval"], json!(true));

    let sources = payload["inventory"]["monitored_sources"]
        .as_array()
        .expect("monitored sources");
    let ci = sources
        .iter()
        .find(|row| row["source_id"].as_str() == Some("ci"))
        .expect("ci source");
    assert!(ci["allowed_destination_ids"]
        .as_array()
        .expect("allowed destinations")
        .iter()
        .any(|destination| destination.as_str() == Some("linear-prod")));

    let keys = payload["inventory"]["scoped_intake_keys"]
        .as_array()
        .expect("intake keys");
    assert_eq!(keys[0]["key_id"], json!("intake-key-authority"));
    assert_eq!(keys[0]["key_hash_present"], json!(true));

    let body_text = String::from_utf8_lossy(&body);
    assert!(!body_text.contains(raw_key));
    assert!(!body_text.contains(&key_hash));
    assert!(!body_text.contains("BUG_MONITOR_AUTHORITY_SECRET"));
    assert!(!body_text.contains("must-not-leak"));
    assert!(!body_text.contains("receipt-must-not-leak"));
    assert!(!body_text.contains("metadata-must-not-leak"));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_authority_inventory_filters_automations_by_request_tenant() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("authority inventory tenant workspace");
    let tenant_a = tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "workspace-a",
        None,
        "actor-a",
    );
    let tenant_b = tandem_types::TenantContext::explicit_user_workspace(
        "org-b",
        "workspace-b",
        None,
        "actor-b",
    );

    let mut automation_a =
        sample_authority_inventory_automation(workspace.path().display().to_string());
    automation_a.automation_id = "tenant-a-authority".to_string();
    automation_a.name = "Tenant A Authority".to_string();
    automation_a.set_tenant_context(&tenant_a);
    state
        .put_automation_v2(automation_a)
        .await
        .expect("tenant a automation");

    let mut automation_b =
        sample_authority_inventory_automation(workspace.path().display().to_string());
    automation_b.automation_id = "tenant-b-authority".to_string();
    automation_b.name = "Tenant B Authority".to_string();
    automation_b.set_tenant_context(&tenant_b);
    state
        .put_automation_v2(automation_b)
        .await
        .expect("tenant b automation");

    let app = app_router(state);
    let resp = app
        .oneshot(authority_inventory_tenant_request(
            "org-a",
            "workspace-a",
            "actor-a",
        ))
        .await
        .expect("authority inventory response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("authority inventory body");
    let payload: Value = serde_json::from_slice(&body).expect("authority inventory json");
    let automations = payload["inventory"]["automation_specs"]
        .as_array()
        .expect("automation specs");
    assert_eq!(payload["counts"]["automation_specs"], json!(1));
    assert!(automations
        .iter()
        .any(|row| row["automation_id"].as_str() == Some("tenant-a-authority")));
    assert!(!automations
        .iter()
        .any(|row| row["automation_id"].as_str() == Some("tenant-b-authority")));
}

#[test]
fn bug_monitor_authority_inventory_dedupes_registry_and_embedded_workflow_hooks() {
    let hook = tandem_workflows::WorkflowHookBinding {
        binding_id: "binding-authority".to_string(),
        workflow_id: "workflow-authority".to_string(),
        event: "incident.created".to_string(),
        enabled: true,
        actions: vec![tandem_workflows::WorkflowActionSpec {
            action: "approval.request".to_string(),
            with: Some(json!({ "secret": "not-returned-as-value" })),
        }],
        source: None,
    };
    let workflow = tandem_workflows::WorkflowSpec {
        workflow_id: "workflow-authority".to_string(),
        name: "Authority workflow".to_string(),
        description: None,
        enabled: true,
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        steps: Vec::new(),
        hooks: vec![hook.clone()],
        source: None,
    };
    let inventory =
        crate::http::bug_monitor::bug_monitor_workflow_inventory(&workflow, &[hook.clone()]);
    let hooks = inventory["hooks"].as_array().expect("workflow hooks");
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0]["binding_id"], json!("binding-authority"));
}

fn authority_inventory_tenant_request(org: &str, workspace: &str, actor: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri("/bug-monitor/security/authority-inventory")
        .header("x-tandem-org-id", org)
        .header("x-tandem-workspace-id", workspace)
        .header("x-tandem-actor-id", actor)
        .body(Body::empty())
        .expect("authority inventory tenant request")
}

fn sample_authority_inventory_automation(workspace_root: String) -> crate::AutomationV2Spec {
    crate::AutomationV2Spec {
        automation_id: "auto-authority-inventory".to_string(),
        name: "Authority Inventory Automation".to_string(),
        description: Some("Exercises write tool and MCP authority inventory".to_string()),
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![crate::AutomationAgentProfile {
            agent_id: "publisher".to_string(),
            template_id: Some("writer-template".to_string()),
            display_name: "Publisher".to_string(),
            avatar_url: None,
            model_policy: Some(json!({
                "default_model": { "provider_id": "openai", "model_id": "gpt-4.1-mini" }
            })),
            skills: vec!["incident_triage".to_string()],
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec!["read".to_string(), "write".to_string()],
                denylist: vec!["shell".to_string()],
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: vec!["github".to_string()],
                allowed_tools: Some(vec!["mcp.github.create_pull_request".to_string()]),
                allowed_connections: Vec::new(),
            },
            approval_policy: Some("publish_requires_human".to_string()),
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![crate::AutomationFlowNode {
                node_id: "publish".to_string(),
                agent_id: "publisher".to_string(),
                objective: "Publish the approved incident follow-up".to_string(),
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: None,
                tool_policy: Some(crate::AutomationAgentToolPolicy {
                    allowlist: vec!["write".to_string()],
                    denylist: Vec::new(),
                }),
                mcp_policy: None,
                retry_policy: None,
                timeout_ms: Some(120_000),
                max_tool_calls: Some(3),
                stage_kind: Some(crate::AutomationNodeStageKind::Approval),
                gate: Some(crate::AutomationApprovalGate {
                    required: true,
                    decisions: vec!["approve".to_string(), "reject".to_string()],
                    rework_targets: vec!["publish".to_string()],
                    instructions: Some("Human approval required before publish".to_string()),
                    expiry_policy: None,
                }),
                metadata: Some(json!({
                    "private_prompt": "metadata-must-not-leak",
                    "owner": "security"
                })),
            }],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_tool_calls: Some(5),
            ..Default::default()
        },
        output_targets: vec!["external:github".to_string()],
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "security-admin".to_string(),
        workspace_root: Some(workspace_root),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}
