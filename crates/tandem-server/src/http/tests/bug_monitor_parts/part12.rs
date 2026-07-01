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
async fn bug_monitor_posture_checks_uses_shared_approval_classifier_for_tools() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("posture classifier workspace");
    let mut automation =
        sample_authority_inventory_automation(workspace.path().display().to_string());
    automation.automation_id = "auto-posture-classifier".to_string();
    automation.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-posture",
        "workspace-posture",
        None,
        "security-admin",
    ));
    automation.agents[0].approval_policy = None;
    automation.agents[0].tool_policy.allowlist = vec![
        "read".to_string(),
        "bash".to_string(),
        "edit".to_string(),
        "patch".to_string(),
        "git_push".to_string(),
    ];
    automation.flow.nodes[0].gate = None;
    automation.flow.nodes[0].tool_policy = None;
    state
        .put_automation_v2(automation)
        .await
        .expect("classifier automation");
    let app = app_router(state);

    let payload = get_bug_monitor_posture_checks_for_tenant(
        app,
        "/bug-monitor/security/posture-checks?rules=high_risk_tool_without_approval",
        "org-posture",
        "workspace-posture",
        "security-admin",
    )
    .await;
    let findings = payload["findings"].as_array().expect("findings");

    assert!(findings.iter().any(|finding| {
        finding["rule_id"].as_str() == Some("high_risk_tool_without_approval")
            && finding["affected_objects"][0]["id"].as_str() == Some("publish")
    }));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_posture_checks_scopes_inherited_agent_allowlist_to_each_node() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("posture scoped node workspace");
    let mut automation =
        sample_authority_inventory_automation(workspace.path().display().to_string());
    automation.automation_id = "auto-posture-node-scope".to_string();
    automation.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-posture",
        "workspace-posture",
        None,
        "security-admin",
    ));
    automation.agents[0].approval_policy = None;
    automation.agents[0].tool_policy.allowlist = vec!["bash".to_string()];
    automation.flow.nodes[0].node_id = "approved".to_string();
    automation.flow.nodes[0].tool_policy = None;
    automation.flow.nodes[0].gate = Some(crate::AutomationApprovalGate {
        required: true,
        decisions: vec!["approve".to_string(), "reject".to_string()],
        rework_targets: vec!["approved".to_string()],
        instructions: Some("Review before running shell".to_string()),
        expiry_policy: None,
    });
    let mut ungated = automation.flow.nodes[0].clone();
    ungated.node_id = "ungated".to_string();
    ungated.gate = None;
    automation.flow.nodes.push(ungated);
    state
        .put_automation_v2(automation)
        .await
        .expect("node scope automation");
    let app = app_router(state);

    let payload = get_bug_monitor_posture_checks_for_tenant(
        app,
        "/bug-monitor/security/posture-checks?rules=high_risk_tool_without_approval",
        "org-posture",
        "workspace-posture",
        "security-admin",
    )
    .await;
    let findings = payload["findings"].as_array().expect("findings");

    assert!(findings.iter().any(|finding| {
        finding["rule_id"].as_str() == Some("high_risk_tool_without_approval")
            && finding["affected_objects"][0]["id"].as_str() == Some("ungated")
    }));
    assert!(!findings.iter().any(|finding| {
        finding["rule_id"].as_str() == Some("high_risk_tool_without_approval")
            && finding["affected_objects"][0]["id"].as_str() == Some("approved")
    }));
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

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_assessment_report_generates_markdown_and_redacted_artifact() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let workspace = tempfile::tempdir().expect("assessment report workspace");
    let mut automation =
        sample_authority_inventory_automation(workspace.path().display().to_string());
    automation.automation_id = "auto-report-risk".to_string();
    automation.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-report",
        "workspace-report",
        None,
        "security-admin",
    ));
    automation.agents[0].approval_policy = None;
    automation.agents[0].mcp_policy.allowed_tools = None;
    automation.flow.nodes[0].gate = None;
    state
        .put_automation_v2(automation)
        .await
        .expect("report automation");
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            destinations: vec![crate::BugMonitorDestinationConfig {
                destination_id: "report-webhook".to_string(),
                name: "Report webhook".to_string(),
                kind: crate::BugMonitorDestinationKind::Webhook,
                enabled: true,
                require_approval: true,
                webhook_url: Some("https://audit.example.test/incidents".to_string()),
                webhook_secret_ref: Some("env:BUG_MONITOR_REPORT_SECRET".to_string()),
                ..Default::default()
            }],
            default_destination_ids: vec!["report-webhook".to_string()],
            ..Default::default()
        })
        .await
        .expect("report config");
    let app = app_router(state);

    let payload = post_bug_monitor_assessment_report(
        app,
        "/bug-monitor/security/assessment-report",
        json!({
            "probes": ["approval_required_tool_policy", "mcp_tool_allowlist"],
            "route_destination_ids": ["report-webhook"]
        }),
        Some(("org-report", "workspace-report", "security-admin", "tk_admin")),
    )
    .await;

    assert_eq!(payload["schema_version"], json!(1));
    assert!(payload["markdown_summary"]
        .as_str()
        .is_some_and(|value| value.contains("Incident Monitor Security Gap Assessment")));
    assert!(payload["sections"]["detected_findings"]["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .any(|finding| finding["rule_id"].as_str()
            == Some("high_risk_tool_without_approval")));
    assert!(payload["sections"]["controlled_probe_results"]["results"]
        .as_array()
        .expect("probe results")
        .iter()
        .any(|result| result["probe_id"].as_str() == Some("mcp_tool_allowlist")
            && result["status"].as_str() == Some("fail")));
    assert_eq!(
        payload["sections"]["external_audit_export"]["route_preview"]["mutates_external_systems"],
        json!(false)
    );
    assert_eq!(payload["evidence_pack"]["persisted"], json!(true));
    let artifact_path = payload["evidence_pack"]["path"]
        .as_str()
        .expect("report artifact path");
    let artifact = std::fs::read_to_string(artifact_path).expect("report artifact");
    assert!(artifact.contains("Incident Monitor Security Gap Assessment"));
    assert!(!artifact.contains("tk_admin"));
    assert!(!artifact.contains("BUG_MONITOR_REPORT_SECRET"));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_assessment_report_distinguishes_self_monitoring_audit_export() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let tenant = tandem_types::TenantContext::explicit_user_workspace(
        "org-audit",
        "workspace-audit",
        None,
        "security-admin",
    );
    let now = crate::now_ms();
    state
        .put_bug_monitor_incident(crate::BugMonitorIncidentRecord {
            incident_id: "incident-self-monitor".to_string(),
            fingerprint: "fp-self".to_string(),
            event_type: "bug_monitor.destination.failure".to_string(),
            status: "open".to_string(),
            repo: "acme/platform".to_string(),
            workspace_root: ".".to_string(),
            title: "Destination publish failed".to_string(),
            source_kind: Some(crate::BugMonitorSourceKind::TandemMonitor),
            source: Some("bug_monitor".to_string()),
            risk_level: Some("high".to_string()),
            risk_category: Some("monitor_health".to_string()),
            tenant_id: Some("org-audit".to_string()),
            workspace_id: Some("workspace-audit".to_string()),
            created_at_ms: now,
            updated_at_ms: now,
            evidence_refs: vec!["protected_audit:bug_monitor.publish.failed".to_string()],
            ..Default::default()
        })
        .await
        .expect("self incident");
    state
        .put_bug_monitor_incident(crate::BugMonitorIncidentRecord {
            incident_id: "incident-external".to_string(),
            fingerprint: "fp-external".to_string(),
            event_type: "external.system.failure".to_string(),
            status: "open".to_string(),
            repo: "acme/platform".to_string(),
            workspace_root: ".".to_string(),
            title: "External system failed".to_string(),
            source_kind: Some(crate::BugMonitorSourceKind::ExternalApp),
            source: Some("customer_app".to_string()),
            tenant_id: Some("org-audit".to_string()),
            workspace_id: Some("workspace-audit".to_string()),
            created_at_ms: now,
            updated_at_ms: now,
            ..Default::default()
        })
        .await
        .expect("external incident");
    state
        .put_bug_monitor_incident(crate::BugMonitorIncidentRecord {
            incident_id: "incident-foreign".to_string(),
            fingerprint: "fp-foreign".to_string(),
            event_type: "bug_monitor.destination.failure".to_string(),
            status: "open".to_string(),
            repo: "acme/platform".to_string(),
            workspace_root: ".".to_string(),
            title: "Foreign tenant incident should stay out".to_string(),
            source_kind: Some(crate::BugMonitorSourceKind::TandemMonitor),
            source: Some("bug_monitor".to_string()),
            risk_level: Some("high".to_string()),
            risk_category: Some("monitor_health".to_string()),
            tenant_id: Some("org-foreign".to_string()),
            workspace_id: Some("workspace-foreign".to_string()),
            created_at_ms: now,
            updated_at_ms: now,
            evidence_refs: vec!["foreign-evidence-ref".to_string()],
            ..Default::default()
        })
        .await
        .expect("foreign incident");
    crate::audit::append_protected_audit_event(
        &state,
        "bug_monitor.publish.failed",
        &tenant,
        None,
        json!({
            "source_kind": "tandem_monitor",
            "destination_id": "audit-webhook",
            "private_value": "super-secret-report-canary"
        }),
    )
    .await
    .expect("protected audit");
    let app = app_router(state);

    let payload = post_bug_monitor_assessment_report(
        app,
        "/failure-reporter/security/assessment-report",
        json!({
            "include_probe_results": false,
            "persist_artifact": false
        }),
        Some(("org-audit", "workspace-audit", "security-admin", "tk_admin")),
    )
    .await;

    let boundary = &payload["sections"]["self_monitoring_boundary"];
    assert_eq!(boundary["self_observed_incidents"], json!(1));
    assert_eq!(boundary["external_system_incidents"], json!(1));
    assert!(boundary["non_claim"]
        .as_str()
        .is_some_and(|value| value.contains("does not independently prove")));
    let records = payload["sections"]["external_audit_export"]["records"]
        .as_array()
        .expect("audit export records");
    assert!(records
        .iter()
        .any(|record| record["event_type"].as_str() == Some("bug_monitor.publish.failed")));
    let payload_string = serde_json::to_string(&payload).expect("report json");
    assert!(!payload_string.contains("Foreign tenant incident should stay out"));
    assert!(!payload_string.contains("foreign-evidence-ref"));
    assert!(!payload_string.contains("org-foreign"));
    assert!(!payload_string.contains("super-secret-report-canary"));
    assert!(payload_string.contains("private_value"));
    assert_eq!(payload["evidence_pack"]["persisted"], json!(false));
}

async fn get_bug_monitor_posture_checks(app: axum::Router, uri: &str) -> Value {
    get_bug_monitor_posture_checks_request(app, uri, None).await
}

async fn post_bug_monitor_assessment_report(
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
                .expect("assessment report request"),
        )
        .await
        .expect("assessment report response");
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("assessment report body");
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&body)
    );
    serde_json::from_slice(&body).expect("assessment report json")
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
