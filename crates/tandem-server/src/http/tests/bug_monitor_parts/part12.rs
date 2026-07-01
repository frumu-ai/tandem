#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_posture_checks_empty_inventory_has_no_findings() {
    let state = test_state().await;
    let app = app_router(state);

    let payload = get_bug_monitor_posture_checks(app, "/bug-monitor/security/posture-checks").await;

    assert_eq!(payload["schema_version"], json!(1));
    assert_eq!(payload["scope"]["read_only"], json!(true));
    assert_eq!(payload["baseline_policy"]["mode"], json!("dry_run"));
    assert_eq!(payload["findings"].as_array().expect("findings").len(), 0);
    assert_eq!(payload["counts"]["findings"], json!(0));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_posture_checks_detects_unapproved_write_tool_and_mcp_gap() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("posture automation workspace");
    let mut automation =
        sample_authority_inventory_automation(workspace.path().display().to_string());
    automation.automation_id = "auto-posture-risky".to_string();
    automation.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-posture",
        "workspace-posture",
        None,
        "security-admin",
    ));
    automation.agents[0].approval_policy = None;
    automation.agents[0].mcp_policy.allowed_tools = None;
    automation.flow.nodes[0].gate = None;
    state
        .put_automation_v2(automation)
        .await
        .expect("risky automation");
    let app = app_router(state);

    let payload = get_bug_monitor_posture_checks_for_tenant(
        app,
        "/bug-monitor/security/posture-checks?rules=high_risk_tool_without_approval,mcp_server_without_tool_allowlist",
        "org-posture",
        "workspace-posture",
        "security-admin",
    )
    .await;
    let findings = payload["findings"].as_array().expect("findings");

    assert!(findings.iter().any(|finding| {
        finding["rule_id"].as_str() == Some("high_risk_tool_without_approval")
            && finding["severity"].as_str() == Some("high")
    }));
    assert!(findings.iter().any(|finding| {
        finding["rule_id"].as_str() == Some("mcp_server_without_tool_allowlist")
            && finding["category"].as_str() == Some("mcp_allowlist_gap")
    }));
    assert_posture_fingerprints_are_unique(findings);
    let body = serde_json::to_string(&payload).expect("payload json");
    assert!(!body.contains("private_prompt"));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_posture_checks_detects_broad_source_and_missing_tenant_context() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("posture source workspace");
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            destinations: vec![
                crate::BugMonitorDestinationConfig {
                    destination_id: "linear-security".to_string(),
                    name: "Linear security".to_string(),
                    kind: crate::BugMonitorDestinationKind::LinearIssue,
                    enabled: true,
                    require_approval: true,
                    linear_team: Some("security".to_string()),
                    ..Default::default()
                },
                crate::BugMonitorDestinationConfig {
                    destination_id: "github-security".to_string(),
                    name: "GitHub security".to_string(),
                    kind: crate::BugMonitorDestinationKind::GithubIssue,
                    enabled: true,
                    require_approval: true,
                    repo: Some("acme/platform".to_string()),
                    ..Default::default()
                },
            ],
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                enabled: true,
                repo: "acme/payments".to_string(),
                workspace_root: workspace.path().display().to_string(),
                source_kind: crate::BugMonitorSourceKind::ExternalApp,
                allowed_destination_ids: Vec::new(),
                tenant_id: None,
                workspace_id: None,
                ..Default::default()
            }],
            safety_defaults: crate::BugMonitorSafetyDefaults {
                block_unready_destinations: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .expect("source config");
    let app = app_router(state);

    let payload = get_bug_monitor_posture_checks(
        app,
        "/failure-reporter/security/posture-checks?rules=broad_source_destination_scope,missing_tenant_context",
    )
    .await;
    let findings = payload["findings"].as_array().expect("findings");

    assert!(findings.iter().any(|finding| {
        finding["rule_id"].as_str() == Some("broad_source_destination_scope")
            && finding["category"].as_str() == Some("scoped_intake_gap")
    }));
    assert!(findings.iter().any(|finding| {
        finding["rule_id"].as_str() == Some("missing_tenant_context")
            && finding["category"].as_str() == Some("tenant_context_gap")
    }));
    assert_posture_fingerprints_are_unique(findings);
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_posture_checks_respects_disabled_mode() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("posture disabled workspace");
    let mut automation =
        sample_authority_inventory_automation(workspace.path().display().to_string());
    automation.automation_id = "auto-posture-disabled-mode".to_string();
    automation.agents[0].approval_policy = None;
    automation.flow.nodes[0].gate = None;
    state
        .put_automation_v2(automation)
        .await
        .expect("disabled mode automation");
    let app = app_router(state);

    let payload = get_bug_monitor_posture_checks(
        app,
        "/bug-monitor/security/posture-checks?mode=disabled",
    )
    .await;

    assert_eq!(payload["baseline_policy"]["mode"], json!("disabled"));
    assert_eq!(payload["findings"].as_array().expect("findings").len(), 0);
    assert!(payload["rules"]
        .as_array()
        .expect("rules")
        .iter()
        .all(|rule| rule["enabled"] == json!(false)));
}

async fn get_bug_monitor_posture_checks(app: axum::Router, uri: &str) -> Value {
    get_bug_monitor_posture_checks_request(app, uri, None).await
}

async fn get_bug_monitor_posture_checks_for_tenant(
    app: axum::Router,
    uri: &str,
    org: &str,
    workspace: &str,
    actor: &str,
) -> Value {
    get_bug_monitor_posture_checks_request(app, uri, Some((org, workspace, actor))).await
}

async fn get_bug_monitor_posture_checks_request(
    app: axum::Router,
    uri: &str,
    tenant: Option<(&str, &str, &str)>,
) -> Value {
    let mut builder = Request::builder().method("GET").uri(uri);
    if let Some((org, workspace, actor)) = tenant {
        builder = builder
            .header("x-tandem-org-id", org)
            .header("x-tandem-workspace-id", workspace)
            .header("x-tandem-actor-id", actor);
    }
    let resp = app
        .oneshot(builder.body(Body::empty()).expect("posture checks request"))
        .await
        .expect("posture checks response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("posture checks body");
    serde_json::from_slice(&body).expect("posture checks json")
}

fn assert_posture_fingerprints_are_unique(findings: &[Value]) {
    let mut seen = std::collections::HashSet::new();
    for finding in findings {
        let fingerprint = finding["fingerprint"]
            .as_str()
            .expect("finding fingerprint");
        assert!(seen.insert(fingerprint.to_string()), "duplicate {fingerprint}");
    }
}
