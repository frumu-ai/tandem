// Governance maturity metrics + drift tests (TAN-488).

fn governance_tenant() -> tandem_types::TenantContext {
    tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "actor-a")
}

fn governance_incident(
    id: &str,
    risk_level: &str,
    evidence: Vec<String>,
    updated_at_ms: u64,
    title: &str,
) -> crate::IncidentMonitorIncidentRecord {
    crate::IncidentMonitorIncidentRecord {
        incident_id: id.to_string(),
        fingerprint: format!("fp-{id}"),
        event_type: "scenario.test".to_string(),
        status: "draft_created".to_string(),
        repo: "acme/platform".to_string(),
        workspace_root: "/tmp/acme".to_string(),
        title: title.to_string(),
        risk_level: Some(risk_level.to_string()),
        risk_category: Some("regulatory".to_string()),
        tenant_id: Some("org-a".to_string()),
        workspace_id: Some("workspace-a".to_string()),
        evidence_refs: evidence,
        occurrence_count: 1,
        created_at_ms: updated_at_ms,
        updated_at_ms,
        ..Default::default()
    }
}

fn governance_decision(
    id: &str,
    effect: tandem_types::PolicyDecisionEffect,
    approval_id: Option<&str>,
    created_at_ms: u64,
) -> tandem_types::PolicyDecisionRecord {
    tandem_types::PolicyDecisionRecord {
        decision_id: id.to_string(),
        tenant_context: governance_tenant(),
        requester_context: None,
        actor_id: Some("actor-a".to_string()),
        session_id: None,
        message_id: None,
        run_id: None,
        automation_id: None,
        node_id: None,
        tool: Some("github.comment".to_string()),
        resource: None,
        data_classes: Vec::new(),
        risk_tier: Some("external_effect".to_string()),
        decision: effect,
        reason_code: "test".to_string(),
        reason: "test".to_string(),
        policy_id: None,
        grant_id: None,
        approval_id: approval_id.map(str::to_string),
        audit_event_id: None,
        created_at_ms,
        metadata: serde_json::json!({}),
    }
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn governance_metrics_compute_confidence_authority_escalation_and_redaction() {
    let state = test_state().await;
    let tenant = governance_tenant();
    let now = crate::now_ms();

    // One high-risk incident with evidence + an auditable receipt (complete), one
    // high-risk incident with neither (incomplete, carrying a secret-ish title).
    state
        .put_incident_monitor_incident(governance_incident(
            "inc-complete",
            "high",
            vec!["evidence://complete".to_string()],
            now,
            "complete case",
        ))
        .await
        .expect("incident");
    state
        .put_incident_monitor_incident(governance_incident(
            "inc-incomplete",
            "critical",
            Vec::new(),
            now,
            "SECRET-TITLE-DO-NOT-LEAK",
        ))
        .await
        .expect("incident");
    state
        .put_incident_monitor_post(crate::IncidentMonitorPostRecord {
            post_id: "post-complete".to_string(),
            draft_id: "draft-complete".to_string(),
            incident_id: Some("inc-complete".to_string()),
            fingerprint: "fp-inc-complete".to_string(),
            repo: "acme/platform".to_string(),
            operation: "create_issue".to_string(),
            status: "posted".to_string(),
            external_id: Some("42".to_string()),
            tenant_id: Some("org-a".to_string()),
            workspace_id: Some("workspace-a".to_string()),
            idempotency_key: "idem-complete".to_string(),
            created_at_ms: now,
            updated_at_ms: now,
            ..Default::default()
        })
        .await
        .expect("post");

    for (id, effect, approval) in [
        ("allow-1", tandem_types::PolicyDecisionEffect::Allow, None),
        ("allow-2", tandem_types::PolicyDecisionEffect::Allow, None),
        ("deny-1", tandem_types::PolicyDecisionEffect::Deny, None),
        (
            "escalate-reached",
            tandem_types::PolicyDecisionEffect::ApprovalRequired,
            Some("approval-1"),
        ),
        (
            "escalate-missed",
            tandem_types::PolicyDecisionEffect::ApprovalRequired,
            None,
        ),
    ] {
        state
            .record_policy_decision(governance_decision(id, effect, approval, now))
            .await
            .expect("decision");
    }

    let thresholds = crate::IncidentMonitorGovernanceThresholds::default();
    let report = crate::incident_monitor_governance_metrics::compute_incident_monitor_governance_metrics(
        &state, &tenant, now, None, &thresholds,
    )
    .await;

    let metric = |id: &str| {
        report["metrics"]
            .as_array()
            .expect("metrics")
            .iter()
            .find(|m| m["metric_id"] == serde_json::json!(id))
            .cloned()
            .unwrap_or_else(|| panic!("missing metric {id}"))
    };

    // governance confidence: 1 of 2 high-risk incidents has a complete trail.
    let confidence = metric("governance_confidence");
    assert_eq!(confidence["numerator"], serde_json::json!(1));
    assert_eq!(confidence["denominator"], serde_json::json!(2));
    assert_eq!(confidence["breached"], serde_json::json!(true));
    assert!(confidence["missing_evidence_reasons"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));

    // authority boundary: 2 allow / (2 allow + 1 deny).
    let authority = metric("authority_boundary_compliance");
    assert_eq!(authority["numerator"], serde_json::json!(2));
    assert_eq!(authority["denominator"], serde_json::json!(3));
    assert_eq!(authority["breached"], serde_json::json!(true));

    // escalation utilization: 1 reached / 2 eligible.
    let escalation = metric("escalation_pathway_utilization");
    assert_eq!(escalation["numerator"], serde_json::json!(1));
    assert_eq!(escalation["denominator"], serde_json::json!(2));
    assert_eq!(escalation["breached"], serde_json::json!(true));

    // Threshold breaches surface as dry-run findings.
    assert!(report["threshold_findings"]
        .as_array()
        .is_some_and(|rows| rows.len() >= 3));
    assert_eq!(report["mutates_external_systems"], serde_json::json!(false));
    assert_eq!(report["redacted"], serde_json::json!(true));

    // Redacted output must not leak the incident title.
    assert!(
        !report.to_string().contains("SECRET-TITLE-DO-NOT-LEAK"),
        "governance metrics must not leak incident free-text"
    );
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn governance_metrics_flag_behavioral_drift_between_windows() {
    let state = test_state().await;
    let tenant = governance_tenant();
    let window_ms = 3_600_000u64; // 1h windows
    let to_ms = 100 * window_ms;
    let current_ts = to_ms - 1_000;
    let baseline_ts = to_ms - window_ms - 1_000;

    // Baseline window: a clean posted receipt (0% failure). Current window: two
    // failed receipts (100% failure) → publish_failure_rate drift.
    for (id, status, ts) in [
        ("baseline-ok", "posted", baseline_ts),
        ("current-fail-1", "failed", current_ts),
        ("current-fail-2", "failed", current_ts),
    ] {
        state
            .put_incident_monitor_post(crate::IncidentMonitorPostRecord {
                post_id: id.to_string(),
                draft_id: format!("draft-{id}"),
                fingerprint: format!("fp-{id}"),
                repo: "acme/platform".to_string(),
                operation: "create_issue".to_string(),
                status: status.to_string(),
                external_id: (status == "posted").then(|| "1".to_string()),
                error: (status == "failed").then(|| "boom".to_string()),
                tenant_id: Some("org-a".to_string()),
                workspace_id: Some("workspace-a".to_string()),
                idempotency_key: format!("idem-{id}"),
                created_at_ms: ts,
                updated_at_ms: ts,
                ..Default::default()
            })
            .await
            .expect("post");
    }

    let thresholds = crate::IncidentMonitorGovernanceThresholds::default();
    let report = crate::incident_monitor_governance_metrics::compute_incident_monitor_governance_metrics(
        &state, &tenant, to_ms, Some(window_ms), &thresholds,
    )
    .await;

    let drift = report["behavioral_drift"]
        .as_array()
        .expect("drift")
        .iter()
        .find(|row| row["signal"] == serde_json::json!("publish_failure_rate"))
        .cloned()
        .expect("publish_failure_rate drift row");
    assert_eq!(drift["current_rate"], serde_json::json!(1.0));
    assert_eq!(drift["baseline_rate"], serde_json::json!(0.0));
    assert_eq!(drift["drift_flagged"], serde_json::json!(true));
    assert!(report["threshold_findings"]
        .as_array()
        .is_some_and(|rows| rows
            .iter()
            .any(|row| row["kind"] == serde_json::json!("behavioral_drift"))));
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn governance_metrics_route_readiness_scopes_to_requesting_tenant() {
    let state = test_state().await;
    let tenant = governance_tenant(); // org-a / workspace-a
    let now = crate::now_ms();
    let workspace_a = tempfile::tempdir().expect("workspace a");
    let workspace_b = tempfile::tempdir().expect("workspace b");

    // Two tenants each own one project + one destination. A tenant-scoped
    // request must count only its own route topology (TAN-547), not the other
    // tenant's enabled sources/destinations.
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            destinations: vec![
                crate::IncidentMonitorDestinationConfig {
                    destination_id: "dest-a".to_string(),
                    name: "Tenant A destination".to_string(),
                    kind: crate::IncidentMonitorDestinationKind::GithubIssue,
                    enabled: true,
                    repo: Some("org-a/platform".to_string()),
                    ..Default::default()
                },
                crate::IncidentMonitorDestinationConfig {
                    destination_id: "dest-b".to_string(),
                    name: "Tenant B destination".to_string(),
                    kind: crate::IncidentMonitorDestinationKind::GithubIssue,
                    enabled: true,
                    repo: Some("org-b/platform".to_string()),
                    ..Default::default()
                },
            ],
            monitored_projects: vec![
                crate::IncidentMonitorMonitoredProject {
                    project_id: "project-a".to_string(),
                    name: "Tenant A".to_string(),
                    enabled: true,
                    repo: "org-a/platform".to_string(),
                    workspace_root: workspace_a.path().display().to_string(),
                    source_kind: crate::IncidentMonitorSourceKind::ExternalApp,
                    allowed_destination_ids: vec!["dest-a".to_string()],
                    tenant_id: Some("org-a".to_string()),
                    workspace_id: Some("workspace-a".to_string()),
                    ..Default::default()
                },
                crate::IncidentMonitorMonitoredProject {
                    project_id: "project-b".to_string(),
                    name: "Tenant B".to_string(),
                    enabled: true,
                    repo: "org-b/platform".to_string(),
                    workspace_root: workspace_b.path().display().to_string(),
                    source_kind: crate::IncidentMonitorSourceKind::ExternalApp,
                    allowed_destination_ids: vec!["dest-b".to_string()],
                    tenant_id: Some("org-b".to_string()),
                    workspace_id: Some("workspace-b".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        })
        .await
        .expect("config");

    let thresholds = crate::IncidentMonitorGovernanceThresholds::default();
    let report = crate::incident_monitor_governance_metrics::compute_incident_monitor_governance_metrics(
        &state, &tenant, now, None, &thresholds,
    )
    .await;

    let readiness = report["metrics"]
        .as_array()
        .expect("metrics")
        .iter()
        .find(|m| m["metric_id"] == serde_json::json!("route_readiness_compliance"))
        .cloned()
        .expect("route_readiness_compliance metric");

    // Only tenant A's destination + source project are counted (2 rows), never
    // tenant B's — and no missing-evidence reason should mention tenant B.
    assert_eq!(
        readiness["denominator"],
        serde_json::json!(2),
        "route readiness must count only the requesting tenant's topology: {readiness}"
    );
    let serialized = readiness.to_string();
    assert!(
        !serialized.contains("dest-b") && !serialized.contains("project-b"),
        "route readiness leaked another tenant's topology: {serialized}"
    );
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn governance_metrics_endpoint_requires_admin_and_is_dry_run() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let app = app_router(state);

    let unauth = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/incident-monitor/security/governance-metrics")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::json!({}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

    let ok = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/incident-monitor/security/governance-metrics")
                .header("content-type", "application/json")
                .header("x-tandem-token", "tk_admin")
                .body(Body::from(serde_json::json!({}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(ok.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(ok.into_body(), usize::MAX).await.expect("body"),
    )
    .expect("json");
    assert_eq!(payload["scope"]["dry_run"], serde_json::json!(true));
    assert_eq!(
        payload["governance_maturity"]["mutates_external_systems"],
        serde_json::json!(false)
    );
    assert!(payload["governance_maturity"]["metrics"]
        .as_array()
        .is_some_and(|rows| rows.len() == 6));
}
