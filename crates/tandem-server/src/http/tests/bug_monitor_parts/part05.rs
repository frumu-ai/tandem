// Source binding tests split from part02.rs for the file-size gate (same module
// via include!).

fn portable_bug_monitor_workspace_root() -> String {
    let root = std::env::temp_dir().join("tandem-bug-monitor-acme");
    std::fs::create_dir_all(root.join("logs")).expect("portable bug monitor workspace");
    root.display().to_string()
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_route_preview_uses_configured_source_binding_and_blocks_disallowed_destination(
) {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    std::fs::create_dir_all(workspace.path().join("logs")).expect("logs dir");
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(workspace.path().display().to_string()),
            destinations: vec![
                crate::BugMonitorDestinationConfig {
                    destination_id: crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string(),
                    name: "Legacy GitHub".to_string(),
                    kind: crate::BugMonitorDestinationKind::GithubIssue,
                    enabled: true,
                    repo: Some("acme/platform".to_string()),
                    ..Default::default()
                },
                crate::BugMonitorDestinationConfig {
                    destination_id: "linear-prod".to_string(),
                    name: "Linear production".to_string(),
                    kind: crate::BugMonitorDestinationKind::LinearIssue,
                    enabled: true,
                    ..Default::default()
                },
            ],
            routes: vec![crate::BugMonitorRouteConfig {
                route_id: "ci-linear".to_string(),
                name: "CI incidents to Linear".to_string(),
                priority: 10,
                destination_ids: vec!["linear-prod".to_string()],
                match_source_kinds: vec!["ci".to_string()],
                match_route_tags: vec!["prod".to_string()],
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
                default_route_tags: vec!["payments".to_string()],
                allowed_destination_ids: vec![
                    crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
                ],
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    source_kind: Some(crate::BugMonitorSourceKind::Ci),
                    default_route_tags: vec!["prod".to_string()],
                    default_destination_ids: vec![
                        crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
                    ],
                    tenant_id: Some("tenant-a".to_string()),
                    workspace_id: Some("workspace-a".to_string()),
                    approval_policy: crate::BugMonitorApprovalPolicy::Always,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let preview_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/route-preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "project_id": "payments",
                "log_source_id": "ci",
                "risk_level": "medium"
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app.oneshot(preview_req).await.expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_payload: Value = serde_json::from_slice(
        &to_bytes(preview_resp.into_body(), usize::MAX)
            .await
            .expect("preview body"),
    )
    .expect("preview json");

    assert_eq!(
        preview_payload
            .get("matches")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("route_id"))
            .and_then(Value::as_str),
        Some("ci-linear")
    );
    assert_eq!(
        preview_payload
            .get("effective_destination_ids")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("linear-prod")
    );
    assert_eq!(
        preview_payload
            .get("approval_required")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        preview_payload
            .get("blocked_reasons")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter().any(|row| {
                    row.as_str()
                        .is_some_and(|reason| reason.contains("not allowed by source binding"))
                })
            })
            .unwrap_or(false),
        "preview should explain source allowlist block: {preview_payload:?}"
    );
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_route_preview_blocks_disjoint_source_allowlist_intersection() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(portable_bug_monitor_workspace_root()),
            destinations: vec![
                crate::BugMonitorDestinationConfig {
                    destination_id: crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string(),
                    name: "Legacy GitHub".to_string(),
                    kind: crate::BugMonitorDestinationKind::GithubIssue,
                    enabled: true,
                    repo: Some("acme/platform".to_string()),
                    ..Default::default()
                },
                crate::BugMonitorDestinationConfig {
                    destination_id: "linear-prod".to_string(),
                    name: "Linear production".to_string(),
                    kind: crate::BugMonitorDestinationKind::LinearIssue,
                    enabled: true,
                    ..Default::default()
                },
            ],
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/platform".to_string(),
                workspace_root: portable_bug_monitor_workspace_root(),
                allowed_destination_ids: vec!["linear-prod".to_string()],
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");
    let draft = crate::BugMonitorDraftRecord {
        draft_id: "failure-draft-source-allowlist-intersection".to_string(),
        fingerprint: "source-allowlist-intersection".to_string(),
        repo: "acme/platform".to_string(),
        project_id: Some("payments".to_string()),
        log_source_id: Some("ci".to_string()),
        status: "draft_ready".to_string(),
        created_at_ms: crate::now_ms(),
        allowed_destination_ids: vec![crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()],
        risk_level: Some("medium".to_string()),
        confidence: Some("medium".to_string()),
        ..Default::default()
    };
    state
        .put_bug_monitor_draft(draft.clone())
        .await
        .expect("draft");

    let app = app_router(state.clone());
    let preview_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/route-preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "draft_id": draft.draft_id,
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app.oneshot(preview_req).await.expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_payload: Value = serde_json::from_slice(
        &to_bytes(preview_resp.into_body(), usize::MAX)
            .await
            .expect("preview body"),
    )
    .expect("preview json");

    assert_eq!(
        preview_payload
            .get("effective_destination_ids")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some(crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID)
    );
    assert!(
        preview_payload
            .get("blocked_reasons")
            .and_then(Value::as_array)
            .is_some_and(|rows| rows.iter().any(|row| {
                row.as_str()
                    .is_some_and(|reason| reason.contains("not allowed by source binding"))
            })),
        "preview should block disjoint source allowlists: {preview_payload:?}"
    );
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_route_preview_blocks_disjoint_project_and_log_source_allowlists() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(portable_bug_monitor_workspace_root()),
            destinations: vec![
                crate::BugMonitorDestinationConfig {
                    destination_id: crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string(),
                    name: "Legacy GitHub".to_string(),
                    kind: crate::BugMonitorDestinationKind::GithubIssue,
                    enabled: true,
                    repo: Some("acme/platform".to_string()),
                    ..Default::default()
                },
                crate::BugMonitorDestinationConfig {
                    destination_id: "linear-prod".to_string(),
                    name: "Linear production".to_string(),
                    kind: crate::BugMonitorDestinationKind::LinearIssue,
                    enabled: true,
                    ..Default::default()
                },
            ],
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/platform".to_string(),
                workspace_root: portable_bug_monitor_workspace_root(),
                allowed_destination_ids: vec!["linear-prod".to_string()],
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    allowed_destination_ids: vec![
                        crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let preview_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/route-preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "project_id": "payments",
                "log_source_id": "ci",
                "risk_level": "medium"
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app.oneshot(preview_req).await.expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_payload: Value = serde_json::from_slice(
        &to_bytes(preview_resp.into_body(), usize::MAX)
            .await
            .expect("preview body"),
    )
    .expect("preview json");

    assert!(
        preview_payload
            .get("blocked_reasons")
            .and_then(Value::as_array)
            .is_some_and(|rows| rows.iter().any(|row| {
                row.as_str()
                    .is_some_and(|reason| reason.contains("not allowed by source binding"))
            })),
        "preview should block disjoint project/source allowlists: {preview_payload:?}"
    );
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_publish_uses_configured_source_binding_for_approval() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(portable_bug_monitor_workspace_root()),
            require_approval_for_new_issues: false,
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/platform".to_string(),
                workspace_root: portable_bug_monitor_workspace_root(),
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    approval_policy: crate::BugMonitorApprovalPolicy::Always,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");
    let draft = crate::BugMonitorDraftRecord {
        draft_id: "failure-draft-source-binding-approval".to_string(),
        fingerprint: "source-binding-approval".to_string(),
        repo: "acme/platform".to_string(),
        project_id: Some("payments".to_string()),
        log_source_id: Some("ci".to_string()),
        status: "draft_ready".to_string(),
        created_at_ms: crate::now_ms(),
        title: Some("Persisted CI draft".to_string()),
        detail: Some("A pre-upgrade draft only persisted project/source IDs.".to_string()),
        confidence: Some("medium".to_string()),
        risk_level: Some("medium".to_string()),
        expected_destination: Some("incident_monitor_issue_draft".to_string()),
        ..Default::default()
    };
    state
        .put_bug_monitor_draft(draft.clone())
        .await
        .expect("draft");

    let app = app_router(state.clone());
    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{}/publish", draft.draft_id))
        .body(Body::empty())
        .expect("publish request");
    let publish_resp = app.oneshot(publish_req).await.expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_payload: Value = serde_json::from_slice(
        &to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body"),
    )
    .expect("publish json");

    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("approval_required")
    );
    let stored = state
        .get_bug_monitor_draft(&draft.draft_id)
        .await
        .expect("stored draft");
    assert_eq!(stored.status, "approval_required");
    assert_eq!(stored.github_status.as_deref(), Some("approval_required"));
    assert!(state.list_bug_monitor_posts(10).await.is_empty());
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_raw_report_cannot_supply_untrusted_never_source_policy() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(portable_bug_monitor_workspace_root()),
            require_approval_for_new_issues: false,
            destinations: vec![crate::BugMonitorDestinationConfig {
                destination_id: crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string(),
                name: "Legacy GitHub".to_string(),
                kind: crate::BugMonitorDestinationKind::GithubIssue,
                enabled: true,
                repo: Some("acme/platform".to_string()),
                require_approval: true,
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let create_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "raw_http",
                    "title": "Untrusted policy report",
                    "detail": "A raw report must not downgrade destination approval.",
                    "risk_level": "medium",
                    "confidence": "medium",
                    "source_approval_policy": "never",
                    "excerpt": ["ERROR raw report failed"]
                }
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let draft_id = create_payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id")
        .to_string();
    let stored = state
        .get_bug_monitor_draft(&draft_id)
        .await
        .expect("stored draft");
    assert!(stored.source_approval_policy.is_none());

    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/publish"))
        .body(Body::empty())
        .expect("publish request");
    let publish_resp = app.oneshot(publish_req).await.expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_payload: Value = serde_json::from_slice(
        &to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body"),
    )
    .expect("publish json");

    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("approval_required")
    );

    let app = app_router(state.clone());
    let anonymous_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "source": "raw_http",
                    "title": "Anonymous raw forged source route report",
                    "detail": "An anonymous raw report must not persist source routing fields.",
                    "risk_level": "medium",
                    "confidence": "medium",
                    "source_kind": "ci",
                    "route_tags": ["forged"],
                    "allowed_destination_ids": ["linear-prod"],
                    "default_destination_ids": ["linear-prod"],
                    "tenant_id": "forged-tenant",
                    "workspace_id": "forged-workspace",
                    "event_schema_version": "forged-v1",
                    "source_approval_policy": "never",
                    "redaction_profile": "forged-redaction",
                    "retention_profile": "forged-retention",
                    "excerpt": ["ERROR anonymous raw forged CI report failed"]
                }
            })
            .to_string(),
        ))
        .expect("anonymous create request");
    let anonymous_resp = app
        .clone()
        .oneshot(anonymous_req)
        .await
        .expect("anonymous create response");
    assert_eq!(anonymous_resp.status(), StatusCode::OK);
    let anonymous_payload: Value = serde_json::from_slice(
        &to_bytes(anonymous_resp.into_body(), usize::MAX)
            .await
            .expect("anonymous create body"),
    )
    .expect("anonymous create json");
    let anonymous_draft_id = anonymous_payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("anonymous draft id")
        .to_string();
    let anonymous_stored = state
        .get_bug_monitor_draft(&anonymous_draft_id)
        .await
        .expect("anonymous stored draft");
    assert!(anonymous_stored.project_id.is_none());
    assert!(anonymous_stored.log_source_id.is_none());
    assert!(anonymous_stored.source_kind.is_none());
    assert!(anonymous_stored.route_tags.is_empty());
    assert!(anonymous_stored.allowed_destination_ids.is_empty());
    assert!(anonymous_stored.default_destination_ids.is_empty());
    assert!(anonymous_stored.tenant_id.is_none());
    assert!(anonymous_stored.workspace_id.is_none());
    assert!(anonymous_stored.event_schema_version.is_none());
    assert!(anonymous_stored.source_approval_policy.is_none());
    assert!(anonymous_stored.redaction_profile.is_none());
    assert!(anonymous_stored.retention_profile.is_none());

    let anonymous_publish_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{anonymous_draft_id}/publish"))
        .body(Body::empty())
        .expect("anonymous publish request");
    let anonymous_publish_resp = app
        .oneshot(anonymous_publish_req)
        .await
        .expect("anonymous publish response");
    assert_eq!(anonymous_publish_resp.status(), StatusCode::OK);
    let anonymous_publish_payload: Value = serde_json::from_slice(
        &to_bytes(anonymous_publish_resp.into_body(), usize::MAX)
            .await
            .expect("anonymous publish body"),
    )
    .expect("anonymous publish json");

    assert_eq!(
        anonymous_publish_payload
            .get("action")
            .and_then(Value::as_str),
        Some("approval_required")
    );
    assert!(state.list_bug_monitor_posts(10).await.is_empty());
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_raw_report_cannot_inherit_configured_never_source_policy() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(portable_bug_monitor_workspace_root()),
            require_approval_for_new_issues: true,
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/platform".to_string(),
                workspace_root: portable_bug_monitor_workspace_root(),
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    approval_policy: crate::BugMonitorApprovalPolicy::Never,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let create_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "project_id": "payments",
                    "log_source_id": "ci",
                    "source": "raw_http",
                    "title": "Raw CI report",
                    "detail": "A raw report must not inherit a source approval exemption.",
                    "risk_level": "medium",
                    "confidence": "medium",
                    "excerpt": ["ERROR raw CI report failed"]
                }
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app.oneshot(create_req).await.expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let draft_id = create_payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id");
    let stored = state
        .get_bug_monitor_draft(draft_id)
        .await
        .expect("stored draft");

    assert_eq!(stored.status, "approval_required");
    assert!(stored.source_approval_policy.is_none());
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_raw_report_cannot_downgrade_global_approval_with_configured_high_risk_policy()
{
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(portable_bug_monitor_workspace_root()),
            require_approval_for_new_issues: true,
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/platform".to_string(),
                workspace_root: portable_bug_monitor_workspace_root(),
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    approval_policy: crate::BugMonitorApprovalPolicy::HighRisk,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let create_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "project_id": "payments",
                    "log_source_id": "ci",
                    "source": "raw_http",
                    "title": "Raw medium CI report",
                    "detail": "A raw report must not downgrade global approval.",
                    "risk_level": "medium",
                    "confidence": "medium",
                    "excerpt": ["ERROR raw medium CI report failed"]
                }
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let draft_id = create_payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id")
        .to_string();
    let stored = state
        .get_bug_monitor_draft(&draft_id)
        .await
        .expect("stored draft");
    assert_eq!(stored.status, "approval_required");
    assert!(stored.source_approval_policy.is_none());

    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/publish"))
        .body(Body::empty())
        .expect("publish request");
    let publish_resp = app.oneshot(publish_req).await.expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_payload: Value = serde_json::from_slice(
        &to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body"),
    )
    .expect("publish json");

    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("approval_required")
    );
    assert!(state.list_bug_monitor_posts(10).await.is_empty());
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_raw_report_clears_untrusted_source_routing_fields() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(portable_bug_monitor_workspace_root()),
            require_approval_for_new_issues: true,
            routes: vec![crate::BugMonitorRouteConfig {
                route_id: "forged-relaxed-route".to_string(),
                name: "Forged relaxed route".to_string(),
                priority: 10,
                destination_ids: vec![crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()],
                match_source_kinds: vec!["ci".to_string()],
                match_route_tags: vec!["forged".to_string()],
                match_tenant_ids: vec!["forged-tenant".to_string()],
                approval_policy: crate::BugMonitorApprovalPolicy::Never,
                ..Default::default()
            }],
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/platform".to_string(),
                workspace_root: portable_bug_monitor_workspace_root(),
                source_kind: crate::BugMonitorSourceKind::ExternalApp,
                default_route_tags: vec!["trusted-payments".to_string()],
                tenant_id: Some("trusted-tenant".to_string()),
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    source_kind: Some(crate::BugMonitorSourceKind::Ci),
                    default_route_tags: vec!["trusted-ci".to_string()],
                    tenant_id: Some("trusted-tenant".to_string()),
                    workspace_id: Some("trusted-workspace".to_string()),
                    default_destination_ids: vec![
                        crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");

    let app = app_router(state.clone());
    let create_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "report": {
                    "project_id": "payments",
                    "log_source_id": "ci",
                    "source": "raw_http",
                    "title": "Raw forged source route report",
                    "detail": "A raw report must not persist source routing fields.",
                    "risk_level": "medium",
                    "confidence": "medium",
                    "source_kind": "ci",
                    "route_tags": ["forged"],
                    "allowed_destination_ids": ["linear-prod"],
                    "default_destination_ids": ["linear-prod"],
                    "tenant_id": "forged-tenant",
                    "workspace_id": "forged-workspace",
                    "event_schema_version": "forged-v1",
                    "source_approval_policy": "never",
                    "redaction_profile": "forged-redaction",
                    "retention_profile": "forged-retention",
                    "excerpt": ["ERROR raw forged CI report failed"]
                }
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let draft_id = create_payload
        .get("draft")
        .and_then(|row| row.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id")
        .to_string();
    let stored = state
        .get_bug_monitor_draft(&draft_id)
        .await
        .expect("stored draft");
    assert_eq!(stored.status, "approval_required");
    assert!(stored.source_kind.is_none());
    assert!(stored.route_tags.is_empty());
    assert!(stored.allowed_destination_ids.is_empty());
    assert!(stored.default_destination_ids.is_empty());
    assert!(stored.tenant_id.is_none());
    assert!(stored.workspace_id.is_none());
    assert!(stored.event_schema_version.is_none());
    assert!(stored.source_approval_policy.is_none());
    assert!(stored.redaction_profile.is_none());
    assert!(stored.retention_profile.is_none());

    let publish_req = Request::builder()
        .method("POST")
        .uri(format!("/bug-monitor/drafts/{draft_id}/publish"))
        .body(Body::empty())
        .expect("publish request");
    let publish_resp = app.oneshot(publish_req).await.expect("publish response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_payload: Value = serde_json::from_slice(
        &to_bytes(publish_resp.into_body(), usize::MAX)
            .await
            .expect("publish body"),
    )
    .expect("publish json");

    assert_eq!(
        publish_payload.get("action").and_then(Value::as_str),
        Some("approval_required")
    );
    assert!(state.list_bug_monitor_posts(10).await.is_empty());
}

#[tokio::test]
async fn bug_monitor_dedupe_keeps_distinct_source_bound_drafts() {
    let state = test_state().await;
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(portable_bug_monitor_workspace_root()),
            ..Default::default()
        })
        .await
        .expect("config");

    let first = state
        .submit_bug_monitor_draft(crate::BugMonitorSubmission {
            repo: Some("acme/platform".to_string()),
            project_id: Some("payments".to_string()),
            log_source_id: Some("api".to_string()),
            fingerprint: Some("shared-source-fingerprint".to_string()),
            source: Some("payments-api".to_string()),
            title: Some("Shared failure fingerprint".to_string()),
            detail: Some("The payments API reported this failure.".to_string()),
            excerpt: vec!["ERROR payments api failed".to_string()],
            ..Default::default()
        })
        .await
        .expect("first draft");
    let second = state
        .submit_bug_monitor_draft(crate::BugMonitorSubmission {
            repo: Some("acme/platform".to_string()),
            project_id: Some("checkout".to_string()),
            log_source_id: Some("worker".to_string()),
            fingerprint: Some("shared-source-fingerprint".to_string()),
            source: Some("checkout-worker".to_string()),
            title: Some("Shared failure fingerprint".to_string()),
            detail: Some("The checkout worker reported this failure.".to_string()),
            excerpt: vec!["ERROR checkout worker failed".to_string()],
            ..Default::default()
        })
        .await
        .expect("second draft");

    assert_ne!(first.draft_id, second.draft_id);
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_scoped_intake_inherits_configured_source_binding() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    std::fs::create_dir_all(workspace.path().join("logs")).expect("logs dir");
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(workspace.path().display().to_string()),
            destinations: vec![crate::BugMonitorDestinationConfig {
                destination_id: crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string(),
                name: "Legacy GitHub".to_string(),
                kind: crate::BugMonitorDestinationKind::GithubIssue,
                enabled: true,
                repo: Some("acme/platform".to_string()),
                ..Default::default()
            }],
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/payments".to_string(),
                workspace_root: workspace.path().display().to_string(),
                source_kind: crate::BugMonitorSourceKind::ExternalApp,
                default_route_tags: vec!["payments".to_string()],
                allowed_destination_ids: vec![
                    crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
                ],
                tenant_id: Some("tenant-a".to_string()),
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    source_kind: Some(crate::BugMonitorSourceKind::Ci),
                    default_route_tags: vec!["ci".to_string()],
                    default_destination_ids: vec![
                        crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
                    ],
                    workspace_id: Some("workspace-a".to_string()),
                    approval_policy: crate::BugMonitorApprovalPolicy::Always,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");
    let raw_key = "tbm_intake_source_binding_test";
    state
        .put_bug_monitor_intake_key(crate::BugMonitorProjectIntakeKey {
            key_id: "intake-key-source-binding".to_string(),
            project_id: "payments".to_string(),
            name: "Payments CI".to_string(),
            key_hash: crate::sha256_hex(&[raw_key]),
            enabled: true,
            scopes: vec!["bug_monitor:report".to_string()],
            created_at_ms: Some(crate::now_ms()),
            last_used_at_ms: None,
        })
        .await
        .expect("intake key");

    let app = app_router(state.clone());
    let intake_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/intake/report")
        .header("content-type", "application/json")
        .header("x-tandem-bug-monitor-intake-key", raw_key)
        .body(Body::from(
            json!({
                "project_id": "payments",
                "source_id": "ci",
                "report": {
                    "title": "CI deploy failed",
                    "detail": "The CI deploy job failed after a policy check.",
                    "source_kind": "customer_system",
                    "route_tags": ["forged"],
                    "allowed_destination_ids": ["linear-prod"],
                    "default_destination_ids": ["linear-prod"],
                    "tenant_id": "forged-tenant",
                    "workspace_id": "forged-workspace",
                    "source_approval_policy": "never",
                    "excerpt": ["ERROR deploy failed"]
                }
            })
            .to_string(),
        ))
        .expect("intake request");
    let intake_resp = app.oneshot(intake_req).await.expect("intake response");
    assert_eq!(intake_resp.status(), StatusCode::OK);
    let intake_payload: Value = serde_json::from_slice(
        &to_bytes(intake_resp.into_body(), usize::MAX)
            .await
            .expect("intake body"),
    )
    .expect("intake json");
    let draft = intake_payload.get("draft").expect("draft");
    let incident = intake_payload.get("incident").expect("incident");

    assert_eq!(
        draft.get("status").and_then(Value::as_str),
        Some("approval_required")
    );
    assert_eq!(draft.get("source_kind").and_then(Value::as_str), Some("ci"));
    assert_eq!(
        draft.get("tenant_id").and_then(Value::as_str),
        Some("tenant-a")
    );
    assert_eq!(
        draft.get("workspace_id").and_then(Value::as_str),
        Some("workspace-a")
    );
    assert_eq!(
        draft
            .get("allowed_destination_ids")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some(crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID)
    );
    assert!(draft
        .get("route_tags")
        .and_then(Value::as_array)
        .is_some_and(|rows| {
            rows.iter().any(|row| row.as_str() == Some("payments"))
                && rows.iter().any(|row| row.as_str() == Some("ci"))
                && !rows.iter().any(|row| row.as_str() == Some("forged"))
        }));
    assert_eq!(
        incident.get("source_kind").and_then(Value::as_str),
        Some("ci")
    );
    assert_eq!(
        incident.get("tenant_id").and_then(Value::as_str),
        Some("tenant-a")
    );
}

#[tokio::test]
#[serial_test::serial(bug_monitor_http)]
async fn bug_monitor_scoped_intake_source_never_overrides_global_approval_default() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("workspace tempdir");
    std::fs::create_dir_all(workspace.path().join("logs")).expect("logs dir");
    state
        .put_bug_monitor_config(crate::BugMonitorConfig {
            enabled: true,
            repo: Some("acme/platform".to_string()),
            workspace_root: Some(workspace.path().display().to_string()),
            require_approval_for_new_issues: true,
            monitored_projects: vec![crate::BugMonitorMonitoredProject {
                project_id: "payments".to_string(),
                name: "Payments".to_string(),
                repo: "acme/payments".to_string(),
                workspace_root: workspace.path().display().to_string(),
                log_sources: vec![crate::BugMonitorLogSource {
                    source_id: "ci".to_string(),
                    path: "logs/ci.jsonl".to_string(),
                    approval_policy: crate::BugMonitorApprovalPolicy::Never,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            default_destination_ids: vec![
                crate::BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()
            ],
            ..Default::default()
        })
        .await
        .expect("config");
    let raw_key = "tbm_intake_source_never_test";
    state
        .put_bug_monitor_intake_key(crate::BugMonitorProjectIntakeKey {
            key_id: "intake-key-source-never".to_string(),
            project_id: "payments".to_string(),
            name: "Payments CI".to_string(),
            key_hash: crate::sha256_hex(&[raw_key]),
            enabled: true,
            scopes: vec!["bug_monitor:report".to_string()],
            created_at_ms: Some(crate::now_ms()),
            last_used_at_ms: None,
        })
        .await
        .expect("intake key");

    let app = app_router(state.clone());
    let intake_req = Request::builder()
        .method("POST")
        .uri("/bug-monitor/intake/report")
        .header("content-type", "application/json")
        .header("x-tandem-bug-monitor-intake-key", raw_key)
        .body(Body::from(
            json!({
                "project_id": "payments",
                "source_id": "ci",
                "report": {
                    "title": "CI deploy failed",
                    "detail": "The CI deploy job failed after a policy check.",
                    "risk_level": "medium",
                    "excerpt": ["ERROR deploy failed"]
                }
            })
            .to_string(),
        ))
        .expect("intake request");
    let intake_resp = app.oneshot(intake_req).await.expect("intake response");
    assert_eq!(intake_resp.status(), StatusCode::OK);
    let intake_payload: Value = serde_json::from_slice(
        &to_bytes(intake_resp.into_body(), usize::MAX)
            .await
            .expect("intake body"),
    )
    .expect("intake json");
    let draft_id = intake_payload
        .get("draft")
        .and_then(|draft| draft.get("draft_id"))
        .and_then(Value::as_str)
        .expect("draft id");
    assert_eq!(
        intake_payload
            .get("draft")
            .and_then(|draft| draft.get("status"))
            .and_then(Value::as_str),
        Some("draft_ready")
    );
    let stored = state
        .get_bug_monitor_draft(draft_id)
        .await
        .expect("stored draft");
    assert_eq!(stored.status, "draft_ready");
    assert_eq!(
        stored.source_approval_policy,
        Some(crate::BugMonitorApprovalPolicy::Never)
    );
}
