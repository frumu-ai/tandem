#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_assessment_probes_detect_approval_and_mcp_gaps() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let workspace = tempfile::tempdir().expect("assessment workspace");
    let mut automation =
        sample_authority_inventory_automation(workspace.path().display().to_string());
    automation.automation_id = "auto-assessment-risk".to_string();
    automation.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-assessment",
        "workspace-assessment",
        None,
        "security-admin",
    ));
    automation.agents[0].approval_policy = None;
    automation.agents[0].mcp_policy.allowed_tools = None;
    automation.flow.nodes[0].gate = None;
    state
        .put_automation_v2(automation)
        .await
        .expect("assessment automation");
    let app = app_router(state);

    let payload = run_bug_monitor_assessment_probes(
        app,
        "/bug-monitor/security/assessment-probes",
        json!({
            "probes": ["approval_required_tool_policy", "mcp_tool_allowlist"]
        }),
        Some(("org-assessment", "workspace-assessment", "security-admin", "tk_admin")),
    )
    .await;
    let results = payload["results"].as_array().expect("probe results");

    assert!(results.iter().any(|result| {
        result["probe_id"].as_str() == Some("approval_required_tool_policy")
            && result["status"].as_str() == Some("fail")
            && result["incident_draft_suggestion"].is_object()
    }));
    assert!(results.iter().any(|result| {
        result["probe_id"].as_str() == Some("mcp_tool_allowlist")
            && result["status"].as_str() == Some("fail")
    }));
    assert_eq!(payload["scope"]["dry_run"], json!(true));
    assert_eq!(payload["evidence_pack"]["persisted"], json!(true));
    let artifact_path = payload["evidence_pack"]["path"]
        .as_str()
        .expect("assessment artifact path");
    assert!(
        std::fs::read_to_string(artifact_path)
            .expect("assessment artifact")
            .contains("approval_required_tool_policy")
    );
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_assessment_probes_reject_scoped_intake_key_and_report_scope() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let workspace = tempfile::tempdir().expect("assessment intake workspace");
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(workspace.path().display().to_string()),
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/payments".to_string(),
                workspace_root: workspace.path().display().to_string(),
                tenant_id: Some("tenant-a".to_string()),
                workspace_id: Some("workspace-a".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        })
        .await
        .expect("assessment intake config");
    let raw_key = "tbm_assessment_intake_report_only";
    state
        .put_bug_monitor_intake_key(crate::BugMonitorProjectIntakeKey {
            key_id: "intake-assessment-report".to_string(),
            project_id: "payments".to_string(),
            name: "Assessment report key".to_string(),
            key_hash: crate::sha256_hex(&[raw_key]),
            enabled: true,
            scopes: vec!["bug_monitor:report".to_string()],
            created_at_ms: Some(crate::now_ms()),
            last_used_at_ms: None,
        })
        .await
        .expect("assessment intake key");
    let app = app_router(state);

    let blocked_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/bug-monitor/security/assessment-probes")
                .header("content-type", "application/json")
                .header("x-tandem-token", "tk_admin")
                .header("x-tandem-bug-monitor-intake-key", raw_key)
                .body(Body::from(
                    json!({"probes": ["scoped_intake_restriction"]}).to_string(),
                ))
                .expect("assessment intake rejected request"),
        )
        .await
        .expect("assessment intake rejected response");
    assert_eq!(blocked_resp.status(), StatusCode::FORBIDDEN);

    let payload = run_bug_monitor_assessment_probes(
        app,
        "/bug-monitor/security/assessment-probes",
        json!({
            "probes": ["scoped_intake_restriction"]
        }),
        Some(("tenant-a", "workspace-a", "security-admin", "tk_admin")),
    )
    .await;
    assert!(payload["results"]
        .as_array()
        .expect("results")
        .iter()
        .any(|result| result["probe_id"].as_str() == Some("scoped_intake_restriction")
            && result["status"].as_str() == Some("pass")));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_assessment_probes_accept_bearer_admin_token() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let app = app_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/bug-monitor/security/assessment-probes")
                .header("content-type", "application/json")
                .header("authorization", "Bearer tk_admin")
                .body(Body::from(
                    json!({"probes": ["destination_readiness_fail_closed"]}).to_string(),
                ))
                .expect("assessment bearer admin request"),
        )
        .await
        .expect("assessment bearer admin response");
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("assessment bearer admin body");

    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let payload: Value = serde_json::from_slice(&body).expect("assessment bearer admin json");
    assert_eq!(payload["scope"]["dry_run"], json!(true));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_assessment_probes_detect_destination_readiness_and_webhook_policy() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            destinations: vec![crate::BugMonitorDestinationConfig {
                destination_id: "private-webhook".to_string(),
                name: "Private webhook".to_string(),
                kind: crate::BugMonitorDestinationKind::Webhook,
                enabled: true,
                require_approval: true,
                webhook_url: Some("http://127.0.0.1:9/incidents".to_string()),
                webhook_secret_ref: Some("env:BUG_MONITOR_ASSESSMENT_WEBHOOK_SECRET".to_string()),
                ..Default::default()
            }],
            default_destination_ids: vec!["private-webhook".to_string()],
            safety_defaults: crate::BugMonitorSafetyDefaults {
                block_unready_destinations: false,
                ..Default::default()
            },
            ..Default::default()
        })
        .await
        .expect("assessment webhook config");
    let app = app_router(state);

    let payload = run_bug_monitor_assessment_probes(
        app,
        "/failure-reporter/security/assessment-probes",
        json!({
            "probes": ["destination_readiness_fail_closed", "webhook_url_policy"]
        }),
        Some(("local", "local", "security-admin", "tk_admin")),
    )
    .await;
    let results = payload["results"].as_array().expect("probe results");

    assert!(results.iter().any(|result| {
        result["probe_id"].as_str() == Some("destination_readiness_fail_closed")
            && result["status"].as_str() == Some("fail")
            && result["observed_behavior"]
                .as_str()
                .is_some_and(|value| value.contains("block_unready_destinations is disabled"))
    }));
    assert!(results.iter().any(|result| {
        result["probe_id"].as_str() == Some("webhook_url_policy")
            && result["status"].as_str() == Some("fail")
            && result["observed_behavior"]
                .as_str()
                .is_some_and(|value| value.contains("localhost/private network")
                    || value.contains("Webhook URL must use https"))
    }));
    let artifact_path = payload["evidence_pack"]["path"]
        .as_str()
        .expect("assessment artifact path");
    let artifact = std::fs::read_to_string(artifact_path).expect("assessment artifact");
    assert!(!artifact.contains("127.0.0.1"));
    assert!(!artifact.contains("BUG_MONITOR_ASSESSMENT_WEBHOOK_SECRET"));
}

async fn run_bug_monitor_assessment_probes(
    app: axum::Router,
    uri: &str,
    input: Value,
    tenant: Option<(&str, &str, &str, &str)>,
) -> Value {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");
    if let Some((org, workspace, actor, token)) = tenant {
        builder = builder
            .header("x-tandem-org-id", org)
            .header("x-tandem-workspace-id", workspace)
            .header("x-tandem-actor-id", actor)
            .header("x-tandem-token", token);
    }
    let resp = app
        .oneshot(
            builder
                .body(Body::from(input.to_string()))
                .expect("assessment probes request"),
        )
        .await
        .expect("assessment probes response");
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("assessment probes body");
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&body)
    );
    serde_json::from_slice(&body).expect("assessment probes json")
}
