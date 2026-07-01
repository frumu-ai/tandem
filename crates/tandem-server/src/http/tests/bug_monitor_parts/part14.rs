#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_deployment_cards_empty_inventory_returns_self_monitoring_card() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let app = app_router(state);

    let payload = post_bug_monitor_deployment_cards(
        app,
        "/bug-monitor/security/deployment-cards",
        json!({}),
        Some(("org-cards", "workspace-cards", "security-admin", "tk_admin")),
    )
    .await;

    let cards = payload["cards"].as_array().expect("deployment cards");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0]["card_id"], json!("tandem:self_monitoring:bug_monitor"));
    assert_eq!(cards[0]["system_boundary"], json!("tandem_self_monitoring"));
    assert!(payload["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .any(|finding| finding["rule_id"].as_str()
            == Some("deployment_card_required_field_missing")
            && finding["affected_objects"][0]["field"].as_str() == Some("business_owner")));
    assert_eq!(payload["scope"]["raw_inventory_included"], json!(false));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_deployment_cards_generate_inventory_cards_and_markdown() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let workspace = tempfile::tempdir().expect("deployment card workspace");
    let mut automation =
        sample_authority_inventory_automation(workspace.path().display().to_string());
    automation.automation_id = "auto-card-payments".to_string();
    automation.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-cards",
        "workspace-cards",
        None,
        "security-admin",
    ));
    state
        .put_automation_v2(automation)
        .await
        .expect("deployment card automation");
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(workspace.path().display().to_string()),
            destinations: vec![crate::BugMonitorDestinationConfig {
                destination_id: "audit-webhook".to_string(),
                name: "Audit webhook".to_string(),
                kind: crate::BugMonitorDestinationKind::Webhook,
                enabled: true,
                require_approval: true,
                webhook_url: Some("https://audit.example.test/incidents".to_string()),
                webhook_secret_ref: Some("env:BUG_MONITOR_CARD_SECRET".to_string()),
                ..Default::default()
            }],
            routes: vec![crate::BugMonitorRouteConfig {
                route_id: "card-route".to_string(),
                name: "Card route".to_string(),
                enabled: true,
                priority: 10,
                destination_ids: vec!["audit-webhook".to_string()],
                match_source_kinds: vec!["external_app".to_string()],
                ..Default::default()
            }],
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/payments".to_string(),
                workspace_root: workspace.path().display().to_string(),
                source_kind: crate::BugMonitorSourceKind::ExternalApp,
                allowed_destination_ids: vec!["audit-webhook".to_string()],
                default_destination_ids: vec!["audit-webhook".to_string()],
                default_route_tags: vec!["payments".to_string()],
                tenant_id: Some("org-cards".to_string()),
                workspace_id: Some("workspace-cards".to_string()),
                event_schema_version: Some("payments.v1".to_string()),
                redaction_profile: Some("standard".to_string()),
                retention_profile: Some("90d".to_string()),
                ..Default::default()
            }],
            default_destination_ids: vec!["audit-webhook".to_string()],
            ..Default::default()
        })
        .await
        .expect("deployment card config");
    let app = app_router(state);

    let payload = post_bug_monitor_deployment_cards(
        app,
        "/failure-reporter/security/deployment-cards",
        json!({
            "include_raw_inventory": true,
            "defaults": {
                "business_owner": "Security Ops",
                "accountable_team": "AI Governance",
                "autonomy_level": "supervised",
                "data_classification": "internal",
                "approval_protocol": "Human approval before high-risk publish",
                "escalation_protocol": "Page security lead for regulatory or legal risk",
                "review_cadence_days": 30,
                "known_limitations": ["Inventory reflects current Tandem configuration"],
                "residual_risks": ["Operator metadata must be reviewed before production"]
            },
            "metadata": {
                "automation:auto-card-payments": {
                    "intended_purpose": "Coordinate approved payments incident follow-up",
                    "evidence_refs": ["runbook:payments-incident-response"]
                },
                "monitored_source:payments:project": {
                    "intended_purpose": "Monitor customer payment incident events"
                }
            }
        }),
        Some(("org-cards", "workspace-cards", "security-admin", "tk_admin")),
    )
    .await;

    let cards = payload["cards"].as_array().expect("deployment cards");
    assert!(cards.iter().any(|card| {
        card["card_id"].as_str() == Some("automation:auto-card-payments")
            && card["business_owner"].as_str() == Some("Security Ops")
            && card["linked_evidence"]["operator_refs"]
                .as_array()
                .is_some_and(|refs| refs.iter().any(|value| value.as_str()
                    == Some("runbook:payments-incident-response")))
    }));
    assert!(cards.iter().any(|card| {
        card["card_id"].as_str() == Some("monitored_source:payments:project")
            && card["system_boundary"].as_str() == Some("customer_owned_system")
            && card["authority"]["inventory"]["redaction_profile_present"].as_bool() == Some(true)
            && card["authority"]["inventory"]["retention_profile_present"].as_bool() == Some(true)
    }));
    assert!(payload["findings"].as_array().expect("findings").is_empty());
    let payload_string = serde_json::to_string(&payload).expect("deployment card json");
    assert!(payload_string.contains("Incident Monitor Deployment Cards"));
    assert!(payload_string.contains("Coordinate approved payments incident follow-up"));
    assert!(!payload_string.contains("BUG_MONITOR_CARD_SECRET"));
    assert!(!payload_string.contains("metadata-must-not-leak"));
    assert_eq!(payload["scope"]["raw_inventory_included"], json!(false));
    assert_eq!(payload["scope"]["raw_inventory_request_ignored"], json!(true));
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_deployment_cards_reject_scoped_intake_key() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let app = app_router(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/bug-monitor/security/deployment-cards")
                .header("content-type", "application/json")
                .header("x-tandem-token", "tk_admin")
                .header("x-tandem-bug-monitor-intake-key", "tbm_card_scoped_key")
                .body(Body::from(json!({}).to_string()))
                .expect("deployment cards scoped intake request"),
        )
        .await
        .expect("deployment cards scoped intake response");
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

async fn post_bug_monitor_deployment_cards(
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
                .expect("deployment cards request"),
        )
        .await
        .expect("deployment cards response");
    let status = resp.status();
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("deployment cards body");
    assert_eq!(
        status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&body)
    );
    serde_json::from_slice(&body).expect("deployment cards json")
}
