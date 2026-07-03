// Continuous reassessment scheduler tests (TAN-490).

use crate::incident_monitor_reassessment::{
    compute_incident_monitor_reassessment, incident_monitor_reassessment_schedule_status,
    incident_monitor_reassessment_scope_key, note_incident_monitor_reassessment_trigger,
    run_due_incident_monitor_reassessments,
};

fn reassessment_tenant() -> tandem_types::TenantContext {
    tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "actor-a")
}

/// Seed an enabled Incident Monitor config with one org-a-bound project whose
/// source lacks readiness/governance metadata, so the posture surfaces produce
/// at least one finding for the reassessment to carry.
async fn seed_reassessment_config(
    state: &crate::AppState,
    workspace: &tempfile::TempDir,
) {
    state
        .put_incident_monitor_config(crate::IncidentMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            destinations: vec![crate::IncidentMonitorDestinationConfig {
                destination_id: "dest-a".to_string(),
                name: "Dest A".to_string(),
                kind: crate::IncidentMonitorDestinationKind::GithubIssue,
                enabled: true,
                repo: Some("acme/platform".to_string()),
                ..Default::default()
            }],
            monitored_projects: vec![crate::IncidentMonitorMonitoredProject {
                project_id: "project-a".to_string(),
                name: "Project A".to_string(),
                enabled: true,
                repo: "acme/platform".to_string(),
                workspace_root: workspace.path().display().to_string(),
                source_kind: crate::IncidentMonitorSourceKind::ExternalApp,
                allowed_destination_ids: vec!["dest-a".to_string()],
                tenant_id: Some("org-a".to_string()),
                workspace_id: Some("workspace-a".to_string()),
                ..Default::default()
            }],
            ..Default::default()
        })
        .await
        .expect("config");
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn reassessment_run_is_versioned_dry_run_and_dedups_findings() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("workspace");
    seed_reassessment_config(&state, &workspace).await;
    let tenant = reassessment_tenant();
    let now = crate::now_ms();

    let first = compute_incident_monitor_reassessment(
        &state,
        &tenant,
        crate::ReassessmentTrigger::Manual,
        None,
        now,
    )
    .await
    .expect("first run");
    let second = compute_incident_monitor_reassessment(
        &state,
        &tenant,
        crate::ReassessmentTrigger::Scheduled,
        None,
        now + 1,
    )
    .await
    .expect("second run");

    // Versioned, dry-run, never mutates external systems.
    assert_eq!(first.version, 1);
    assert_eq!(second.version, 2);
    assert_eq!(second.mode, "dry_run");
    assert!(!second.mutates_external_systems);
    assert!(!first.findings.is_empty(), "posture should produce findings");

    // Stable fingerprints suppress duplicate noise: the same posture is
    // recurring on the second run, not new, and occurrence counts climb.
    assert_eq!(
        second.comparison.previous_version,
        Some(1),
        "second run compares to the first"
    );
    assert!(
        second.comparison.new_fingerprints.is_empty(),
        "unchanged posture yields no new findings on rerun: {:?}",
        second.comparison.new_fingerprints
    );
    assert!(!second.comparison.recurring_fingerprints.is_empty());
    assert!(second
        .findings
        .iter()
        .all(|finding| finding.occurrence_count >= 2 && finding.status == "recurring"));

    // Redacted: the workspace path and other free text never leak.
    let serialized = serde_json::to_string(&second).expect("serialize");
    assert!(
        !serialized.contains(&workspace.path().display().to_string()),
        "reassessment record must not leak the workspace path"
    );
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn reassessment_config_change_enqueues_pending_and_run_consumes_it() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("workspace");
    let tenant = reassessment_tenant();
    let scope_key = incident_monitor_reassessment_scope_key(&tenant, "deployment");

    // Editing config (adding routes/sources/tenant binding) schedules a
    // change-triggered reassessment for the affected tenant scope.
    seed_reassessment_config(&state, &workspace).await;
    let pending = state
        .incident_monitor_reassessment_pending
        .read()
        .await
        .get(&scope_key)
        .cloned();
    let pending = pending.expect("config change enqueues a pending reassessment");
    assert!(
        pending.trigger.is_change_event(),
        "pending trigger is a change event: {:?}",
        pending.trigger
    );

    // The scheduler tick runs the due scope with the change trigger recorded and
    // clears the pending marker.
    let produced = run_due_incident_monitor_reassessments(&state, crate::now_ms())
        .await
        .expect("run due");
    assert!(produced.iter().any(|record| record.scope_key == scope_key));
    let record = produced
        .into_iter()
        .find(|record| record.scope_key == scope_key)
        .expect("record for scope");
    assert_ne!(record.trigger, "scheduled", "ran on the change trigger");
    assert!(state
        .incident_monitor_reassessment_pending
        .read()
        .await
        .get(&scope_key)
        .is_none());
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn reassessment_workflow_authority_trigger_enqueues_and_runs() {
    let state = test_state().await;
    // Local/implicit deployment: no config needed. The generic trigger entry is
    // what non-config subsystems (workflow/agent authority, MCP inventory) call.
    let tenant = tandem_types::TenantContext::local_implicit();
    let scope_key = incident_monitor_reassessment_scope_key(&tenant, "deployment");

    note_incident_monitor_reassessment_trigger(
        &state,
        &tenant,
        crate::ReassessmentTrigger::WorkflowAuthorityChange,
        Some("workflow authority changed".to_string()),
        crate::now_ms(),
    )
    .await;
    assert_eq!(
        state
            .incident_monitor_reassessment_pending
            .read()
            .await
            .get(&scope_key)
            .map(|pending| pending.trigger),
        Some(crate::ReassessmentTrigger::WorkflowAuthorityChange)
    );

    let produced = run_due_incident_monitor_reassessments(&state, crate::now_ms())
        .await
        .expect("run due");
    let record = produced
        .into_iter()
        .find(|record| record.scope_key == scope_key)
        .expect("record for local scope");
    assert_eq!(record.trigger, "workflow_authority_change");
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn reassessment_schedule_status_reports_overdue() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("workspace");
    seed_reassessment_config(&state, &workspace).await;
    let tenant = reassessment_tenant();
    let scope_key = incident_monitor_reassessment_scope_key(&tenant, "deployment");

    // Run once well in the past, then read status far in the future.
    let long_ago = 1_000_000_000_000u64;
    let day_ms = 24 * 60 * 60 * 1_000u64;
    compute_incident_monitor_reassessment(
        &state,
        &tenant,
        crate::ReassessmentTrigger::Scheduled,
        None,
        long_ago,
    )
    .await
    .expect("historical run");

    let now = long_ago + 30 * day_ms;
    let schedule = incident_monitor_reassessment_schedule_status(&state, &tenant, now).await;
    let deployment = schedule
        .iter()
        .find(|status| status.scope_key == scope_key)
        .expect("deployment scope status");
    assert_eq!(deployment.last_completed_at_ms, Some(long_ago));
    assert_eq!(deployment.next_due_at_ms, long_ago + 7 * day_ms);
    assert!(deployment.overdue, "30d past a 7d cadence is overdue");
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn reassessment_endpoint_requires_admin_and_is_dry_run() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let app = app_router(state);

    let unauth = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/incident-monitor/security/reassessments")
                .header("content-type", "application/json")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

    let ok = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/incident-monitor/security/reassessments")
                .header("x-tandem-token", "tk_admin")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(ok.status(), StatusCode::OK);
    let payload: Value =
        serde_json::from_slice(&to_bytes(ok.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    assert_eq!(payload["scope"]["dry_run"], serde_json::json!(true));
    assert_eq!(
        payload["reassessment"]["mutates_external_systems"],
        serde_json::json!(false)
    );
    assert_eq!(payload["reassessment"]["version"], serde_json::json!(1));
    assert!(payload["schedule"].is_array());

    // The list endpoint reports the run + schedule status.
    let listed = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/incident-monitor/security/reassessments")
                .header("x-tandem-token", "tk_admin")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_payload: Value =
        serde_json::from_slice(&to_bytes(listed.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    assert!(listed_payload["records"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));
    assert!(listed_payload["schedule"].is_array());
}

#[tokio::test]
#[serial_test::serial(incident_monitor_http)]
async fn reassessment_schedule_surfaces_on_deployment_cards() {
    let state = test_state().await;
    state.set_api_token(Some("tk_admin".to_string())).await;
    let app = app_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/incident-monitor/security/deployment-cards")
                .header("content-type", "application/json")
                .header("x-tandem-token", "tk_admin")
                .body(Body::from(serde_json::json!({}).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    let payload: Value = serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .expect("json");
    // Live reassessment schedule status is attached to the cards payload.
    assert_eq!(
        payload["reassessment"]["source"],
        serde_json::json!("incident_monitor_continuous_reassessment")
    );
    assert!(payload["reassessment"]["schedule"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));
}
