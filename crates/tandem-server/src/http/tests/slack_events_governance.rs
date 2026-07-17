// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Governance-focused Slack ingress tests for the TAN-762..TAN-766 stack:
//! multi-channel connection routing, department intersection scoping,
//! department-binding enrollment, sender discovery, and per-connection
//! binding verification. Shares the signed-events harness (mock Slack API,
//! signed requests, governed identity seeding) with `slack_events`.

use super::*;

use super::slack_events::{
    configure_slack_events, configure_slack_events_for_installation,
    install_governed_slack_provider, seed_governed_slack_identity,
    seed_governed_slack_identity_for_user, signed_slack_event_request,
    signed_slack_event_request_for_installation, start_slack_api_mock, wait_for_posts,
    wait_for_slack_tasks, ORG_ID, SIGNING_SECRET, SLACK_APP, SLACK_CHANNEL, SLACK_TEAM, SLACK_USER,
    WORKSPACE_ID,
};
use std::sync::atomic::Ordering;
use tandem_types::{
    AccessPermission, DataClass, OrganizationUnit, OrganizationUnitAccessGrant,
    OrganizationUnitKind, PrincipalRef, ResourceKind, ResourceRef, TenantContext,
};

/// Configure two per-channel connections (TAN-763) sharing one installation:
/// installation identity, signing secret, bot token, tenant, and model come
/// from the top level; each connection sets its own channel + allowlist.
async fn configure_slack_event_connections(
    state: &AppState,
    api_base_url: &str,
    sales_channel: &str,
    sales_user: &str,
    eng_channel: &str,
    eng_user: &str,
) {
    state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "signing_secret": SIGNING_SECRET,
                    "events_enabled": true,
                    "bot_token": "xoxb-governed-test",
                    "team_id": SLACK_TEAM,
                    "app_id": SLACK_APP,
                    "api_base_url": api_base_url,
                    "model_provider_id": "governed-slack-test",
                    "model_id": "governed-slack-test-1",
                    "security_profile": "trusted_team",
                    "tenant": {
                        "org_id": ORG_ID,
                        "workspace_id": WORKSPACE_ID
                    },
                    "connections": [
                        {
                            "channel_id": sales_channel,
                            "allowed_users": [sales_user]
                        },
                        {
                            "channel_id": eng_channel,
                            "allowed_users": [eng_user]
                        }
                    ]
                }
            }
        }))
        .await
        .expect("configure Slack Events connections");
}

/// Seed only the engineering department unit + grant — no memberships. Used
/// by TAN-765 tests where the membership must come from enrollment.
async fn seed_engineering_unit_and_grant(state: &AppState, tool_patterns: &[&str]) {
    let now_ms = crate::now_ms();
    let tenant = TenantContext::explicit(ORG_ID, WORKSPACE_ID, None);
    let admin = PrincipalRef::human_user("admin");
    let department = OrganizationUnit::active(
        "engineering",
        tenant.clone(),
        "Engineering",
        OrganizationUnitKind::Department,
        admin,
        now_ms,
    )
    .with_taxonomy_id("department");
    let grant = OrganizationUnitAccessGrant::active(
        "engineering-read",
        tenant,
        department.principal_ref(),
        ResourceRef::new(ORG_ID, WORKSPACE_ID, ResourceKind::Workspace, WORKSPACE_ID),
        now_ms,
    )
    .with_permissions(vec![AccessPermission::Read, AccessPermission::Execute])
    .with_data_classes(vec![DataClass::Internal, DataClass::SourceCode])
    .with_tool_patterns(
        tool_patterns
            .iter()
            .map(|pattern| (*pattern).to_string())
            .collect(),
    );
    state
        .enterprise
        .org_units
        .write()
        .await
        .insert(department.unit_id.clone(), department);
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert(grant.grant_id.clone(), grant);
}

#[tokio::test]
async fn multi_connection_events_route_to_their_own_channels() {
    const SALES_CHANNEL: &str = "C_SALES";
    const ENG_CHANNEL: &str = "C_ENG";
    const SALES_USER: &str = "U_SALES";
    const ENG_USER: &str = "U_ENG";

    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    configure_slack_event_connections(
        &state,
        &api_base_url,
        SALES_CHANNEL,
        SALES_USER,
        ENG_CHANNEL,
        ENG_USER,
    )
    .await;
    seed_governed_slack_identity_for_user(&state, SALES_USER, &["mcp.crm.*"]).await;
    seed_governed_slack_identity_for_user(&state, ENG_USER, &["mcp.github.*"]).await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let request_timestamp = chrono::Utc::now().timestamp();

    // Each connection accepts its own channel's events and replies in place.
    let sales_event = signed_slack_event_request_for_installation(
        "Ev-conn-sales-1",
        SALES_USER,
        SALES_CHANNEL,
        Some(SLACK_TEAM),
        Some(SLACK_APP),
        "1800000000.310001",
        None,
        request_timestamp,
    );
    let response = app
        .clone()
        .oneshot(sales_event)
        .await
        .expect("sales response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 1).await;

    let eng_event = signed_slack_event_request_for_installation(
        "Ev-conn-eng-1",
        ENG_USER,
        ENG_CHANNEL,
        Some(SLACK_TEAM),
        Some(SLACK_APP),
        "1800000000.320001",
        None,
        request_timestamp,
    );
    let response = app.clone().oneshot(eng_event).await.expect("eng response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 2).await;
    wait_for_slack_tasks(&state).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);

    let posts = slack_mock.posts.lock().await;
    assert_eq!(posts[0]["channel"], SALES_CHANNEL);
    assert_eq!(posts[1]["channel"], ENG_CHANNEL);
    drop(posts);

    let mut scope_ids = state
        .storage
        .list_sessions()
        .await
        .into_iter()
        .filter_map(|session| {
            session
                .source_metadata
                .as_ref()
                .and_then(|metadata| metadata.get("scope_id"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    scope_ids.sort();
    assert_eq!(
        scope_ids,
        vec![
            format!("thread:{SLACK_TEAM}:{SLACK_APP}:{ENG_CHANNEL}:1800000000.320001"),
            format!("thread:{SLACK_TEAM}:{SLACK_APP}:{SALES_CHANNEL}:1800000000.310001"),
        ],
        "each connection keys its own session scope"
    );

    // A sender authorized on one connection is NOT authorized on the other:
    // per-connection allowlists must not pool.
    let cross_connection = signed_slack_event_request_for_installation(
        "Ev-conn-cross-1",
        SALES_USER,
        ENG_CHANNEL,
        Some(SLACK_TEAM),
        Some(SLACK_APP),
        "1800000000.330001",
        None,
        request_timestamp,
    );
    let response = app
        .clone()
        .oneshot(cross_connection)
        .await
        .expect("cross-connection response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // A channel no connection claims stays rejected.
    let unknown_channel = signed_slack_event_request_for_installation(
        "Ev-conn-unknown-1",
        SALES_USER,
        "C_UNCONFIGURED",
        Some(SLACK_TEAM),
        Some(SLACK_APP),
        "1800000000.340001",
        None,
        request_timestamp,
    );
    let response = app
        .oneshot(unknown_channel)
        .await
        .expect("unknown channel response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        2,
        "denied events must not dispatch model runs"
    );
    assert_eq!(slack_mock.posts.lock().await.len(), 2);
    mock_task.abort();
}

async fn configure_slack_events_with_bound_departments(
    state: &AppState,
    api_base_url: &str,
    org_units: &[&str],
) {
    state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "signing_secret": SIGNING_SECRET,
                    "events_enabled": true,
                    "bot_token": "xoxb-governed-test",
                    "channel_id": SLACK_CHANNEL,
                    "team_id": SLACK_TEAM,
                    "app_id": SLACK_APP,
                    "allowed_users": [SLACK_USER],
                    "api_base_url": api_base_url,
                    "model_provider_id": "governed-slack-test",
                    "model_id": "governed-slack-test-1",
                    "security_profile": "trusted_team",
                    "org_units": org_units,
                    "tenant": {
                        "org_id": ORG_ID,
                        "workspace_id": WORKSPACE_ID
                    }
                }
            }
        }))
        .await
        .expect("configure department-bound Slack Events");
}

#[tokio::test]
async fn department_bound_connection_narrows_run_authority_to_intersection() {
    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    // The user holds department/engineering AND role/engineer; the channel
    // binds only the department, so the run must narrow to it.
    configure_slack_events_with_bound_departments(
        &state,
        &api_base_url,
        &["department/engineering"],
    )
    .await;
    seed_governed_slack_identity(&state).await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let request_timestamp = chrono::Utc::now().timestamp();

    let event = signed_slack_event_request(
        "Ev-dept-narrow-1",
        SLACK_USER,
        "1800000000.410001",
        None,
        request_timestamp,
    );
    let response = app.oneshot(event).await.expect("narrowed response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 1).await;
    wait_for_slack_tasks(&state).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    let sessions = state.storage.list_sessions().await;
    assert_eq!(sessions.len(), 1);
    let verified = sessions[0]
        .verified_tenant_context
        .as_ref()
        .expect("verified channel context");
    assert_eq!(
        verified.org_units,
        vec!["department/engineering"],
        "run authority must narrow to the intersection"
    );
    assert!(
        verified.roles.is_empty(),
        "role units outside the channel binding must not survive the intersection"
    );
    assert_eq!(
        verified.capabilities,
        vec!["mcp.github.*"],
        "grants sourced from the bound department are kept"
    );

    let audit_tenant = TenantContext::explicit(ORG_ID, WORKSPACE_ID, None);
    let audit = crate::audit::load_protected_audit_events_for_tenant(&state, &audit_tenant).await;
    let run_started = audit
        .iter()
        .find(|event| event.event_type == "channel.slack.run.started")
        .expect("run.started audit event");
    assert_eq!(
        run_started.payload.get("channel_org_units"),
        Some(&json!(["department/engineering"])),
        "receipts must show the channel's binding alongside the effective units"
    );
    assert_eq!(
        run_started.payload.get("org_units"),
        Some(&json!(["department/engineering"]))
    );
    mock_task.abort();
}

#[tokio::test]
async fn enrollment_code_with_department_binding_enables_governed_run() {
    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    configure_slack_events(&state, &api_base_url).await;
    // Department + grant exist, but the sender has NO membership yet.
    seed_engineering_unit_and_grant(&state, &["mcp.github.*"]).await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let request_timestamp = chrono::Utc::now().timestamp();

    // Unmapped sender fails closed.
    let before = signed_slack_event_request(
        "Ev-enroll-before",
        SLACK_USER,
        "1800000000.510001",
        None,
        request_timestamp,
    );
    let response = app
        .clone()
        .oneshot(before)
        .await
        .expect("pre-enrollment response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);

    // Operator issues a pairing code carrying the department binding; the
    // identity redeems it (TAN-765). An unknown unit fails at issue time.
    let principal = format!("channel:slack:{SLACK_TEAM}:{SLACK_APP}:{SLACK_USER}");
    assert!(
        state
            .issue_channel_enrollment_code(
                "slack",
                principal.clone(),
                crate::app::state::channel_user_capabilities::StoredCommandTier::Approve,
                Some(60_000),
                Some("operator".to_string()),
                None,
                vec!["department/nonexistent".to_string()],
                None,
            )
            .await
            .is_err(),
        "unknown org unit must fail at issue time"
    );
    let code = state
        .issue_channel_enrollment_code(
            "slack",
            principal.clone(),
            crate::app::state::channel_user_capabilities::StoredCommandTier::Approve,
            Some(60_000),
            Some("operator".to_string()),
            None,
            vec!["department/engineering".to_string()],
            None,
        )
        .await
        .expect("issue department-bound enrollment code");
    let capability = state
        .confirm_channel_enrollment_code(&code.code, None)
        .await
        .expect("confirm department-bound enrollment code");
    assert_eq!(capability.org_units, vec!["department/engineering"]);
    assert!(
        state
            .enterprise
            .org_unit_memberships
            .read()
            .await
            .values()
            .any(|membership| {
                membership.member.id == principal
                    && membership.unit.id == "department/engineering"
                    && membership.state.is_active()
            }),
        "confirming the code must establish the org-unit membership"
    );

    // The same sender now runs governed, scoped to the enrolled department.
    let after = signed_slack_event_request(
        "Ev-enroll-after",
        SLACK_USER,
        "1800000000.520001",
        None,
        request_timestamp,
    );
    let response = app.oneshot(after).await.expect("post-enrollment response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 1).await;
    wait_for_slack_tasks(&state).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    let sessions = state.storage.list_sessions().await;
    let verified = sessions[0]
        .verified_tenant_context
        .as_ref()
        .expect("verified channel context");
    assert_eq!(verified.org_units, vec!["department/engineering"]);
    assert_eq!(verified.capabilities, vec!["mcp.github.*"]);
    mock_task.abort();
}

#[tokio::test]
async fn slack_senders_endpoint_surfaces_mapped_and_unmapped_identities() {
    const UNMAPPED_USER: &str = "U_UNMAPPED";

    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    configure_slack_events_for_installation(
        &state,
        &api_base_url,
        SLACK_TEAM,
        SLACK_APP,
        SLACK_CHANNEL,
        &[SLACK_USER, UNMAPPED_USER],
    )
    .await;
    seed_governed_slack_identity(&state).await;
    let _provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let request_timestamp = chrono::Utc::now().timestamp();

    // Mapped sender produces an accepted run; unmapped sender an audited
    // fail-closed denial.
    let accepted = signed_slack_event_request(
        "Ev-senders-accepted",
        SLACK_USER,
        "1800000000.610001",
        None,
        request_timestamp,
    );
    let response = app
        .clone()
        .oneshot(accepted)
        .await
        .expect("accepted response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 1).await;
    wait_for_slack_tasks(&state).await;

    let denied = signed_slack_event_request(
        "Ev-senders-denied",
        UNMAPPED_USER,
        "1800000000.620001",
        None,
        request_timestamp,
    );
    let response = app.clone().oneshot(denied).await.expect("denied response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let request = Request::builder()
        .method("GET")
        .uri("/channels/slack/senders")
        .body(Body::empty())
        .expect("senders request");
    let response = app.oneshot(request).await.expect("senders response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("senders body");
    let payload: Value = serde_json::from_slice(&body).expect("senders json");
    let senders = payload
        .get("senders")
        .and_then(Value::as_array)
        .expect("senders array");

    let mapped = senders
        .iter()
        .find(|row| row.get("user_id").and_then(Value::as_str) == Some(SLACK_USER))
        .expect("mapped sender present");
    assert_eq!(mapped.get("mapped"), Some(&json!(true)));
    assert_eq!(
        mapped.get("principal").and_then(Value::as_str),
        Some(format!("channel:slack:{SLACK_TEAM}:{SLACK_APP}:{SLACK_USER}").as_str())
    );
    assert!(mapped
        .get("org_units")
        .and_then(Value::as_array)
        .expect("mapped org units")
        .iter()
        .any(|unit| unit == "department/engineering"));
    assert!(
        mapped
            .get("accepted_count")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            >= 1
    );

    let unmapped = senders
        .iter()
        .find(|row| row.get("user_id").and_then(Value::as_str) == Some(UNMAPPED_USER))
        .expect("unmapped sender present — denials must be visible");
    assert_eq!(unmapped.get("mapped"), Some(&json!(false)));
    assert!(
        unmapped
            .get("denied_count")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            >= 1
    );
    assert!(
        unmapped
            .get("last_denial_reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("organization-unit membership"),
        "unmapped denial reason must explain the fail-closed state"
    );
    mock_task.abort();
}

#[tokio::test]
async fn denials_are_audited_under_a_connection_bound_tenant() {
    let state = test_state().await;
    let (api_base_url, _slack_mock, mock_task) = start_slack_api_mock().await;
    // The tenant lives ONLY on the connection entry — no top-level binding.
    // Fail-closed denials must still land in the protected audit ledger so
    // the unmapped sender stays discoverable via /channels/slack/senders.
    state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "signing_secret": SIGNING_SECRET,
                    "events_enabled": true,
                    "bot_token": "xoxb-governed-test",
                    "team_id": SLACK_TEAM,
                    "app_id": SLACK_APP,
                    "api_base_url": api_base_url,
                    "model_provider_id": "governed-slack-test",
                    "model_id": "governed-slack-test-1",
                    "security_profile": "trusted_team",
                    "connections": [
                        {
                            "channel_id": SLACK_CHANNEL,
                            "allowed_users": [SLACK_USER],
                            "tenant": {
                                "org_id": ORG_ID,
                                "workspace_id": WORKSPACE_ID
                            }
                        }
                    ]
                }
            }
        }))
        .await
        .expect("configure connection-tenant Slack Events");
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let request_timestamp = chrono::Utc::now().timestamp();

    // Allowlisted but holding no org-unit membership: denied in governed
    // identity resolution — the exact denial an operator must be able to see.
    let denied = signed_slack_event_request(
        "Ev-conn-tenant-denied",
        SLACK_USER,
        "1800000000.710001",
        None,
        request_timestamp,
    );
    let response = app.clone().oneshot(denied).await.expect("denied response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);

    let audit_tenant = TenantContext::explicit(ORG_ID, WORKSPACE_ID, None);
    let audit = crate::audit::load_protected_audit_events_for_tenant(&state, &audit_tenant).await;
    assert!(
        audit
            .iter()
            .any(|event| event.event_type == "channel.slack.ingress.denied"),
        "the denial must be audited under the connection's bound tenant"
    );

    let request = Request::builder()
        .method("GET")
        .uri("/channels/slack/senders")
        .body(Body::empty())
        .expect("senders request");
    let response = app.oneshot(request).await.expect("senders response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("senders body");
    let payload: Value = serde_json::from_slice(&body).expect("senders json");
    let row = payload
        .get("senders")
        .and_then(Value::as_array)
        .expect("senders array")
        .iter()
        .find(|row| row.get("user_id").and_then(Value::as_str) == Some(SLACK_USER))
        .cloned()
        .expect("denied sender must be discoverable with a connection-only tenant");
    assert_eq!(row.get("mapped"), Some(&json!(false)));
    assert!(
        row.get("denied_count")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            >= 1
    );
    mock_task.abort();
}

#[tokio::test]
async fn sender_mapping_is_computed_against_each_channels_department_binding() {
    const SALES_CHANNEL: &str = "C_SALES_MAP";
    const ENG_CHANNEL: &str = "C_ENG_MAP";

    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "signing_secret": SIGNING_SECRET,
                    "events_enabled": true,
                    "bot_token": "xoxb-governed-test",
                    "team_id": SLACK_TEAM,
                    "app_id": SLACK_APP,
                    "api_base_url": api_base_url,
                    "model_provider_id": "governed-slack-test",
                    "model_id": "governed-slack-test-1",
                    "security_profile": "trusted_team",
                    "tenant": {
                        "org_id": ORG_ID,
                        "workspace_id": WORKSPACE_ID
                    },
                    "connections": [
                        {
                            "channel_id": ENG_CHANNEL,
                            "allowed_users": [SLACK_USER],
                            "org_units": ["engineering"]
                        },
                        {
                            "channel_id": SALES_CHANNEL,
                            "allowed_users": [SLACK_USER],
                            "org_units": ["sales"]
                        }
                    ]
                }
            }
        }))
        .await
        .expect("configure department-bound connections");
    // The sender holds an engineering membership only.
    seed_governed_slack_identity(&state).await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let request_timestamp = chrono::Utc::now().timestamp();

    // The engineering-bound channel accepts the engineering member…
    let accepted = signed_slack_event_request_for_installation(
        "Ev-perch-eng",
        SLACK_USER,
        ENG_CHANNEL,
        Some(SLACK_TEAM),
        Some(SLACK_APP),
        "1800000000.810001",
        None,
        request_timestamp,
    );
    let response = app.clone().oneshot(accepted).await.expect("eng response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 1).await;
    wait_for_slack_tasks(&state).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    // …and the sales-bound channel fails the same sender closed.
    let denied = signed_slack_event_request_for_installation(
        "Ev-perch-sales",
        SLACK_USER,
        SALES_CHANNEL,
        Some(SLACK_TEAM),
        Some(SLACK_APP),
        "1800000000.820001",
        None,
        request_timestamp,
    );
    let response = app.clone().oneshot(denied).await.expect("sales response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Sender discovery must not let the engineering membership mask the
    // sales-channel gap: mapped is per observed channel, not tenant-wide.
    let request = Request::builder()
        .method("GET")
        .uri("/channels/slack/senders")
        .body(Body::empty())
        .expect("senders request");
    let response = app.oneshot(request).await.expect("senders response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("senders body");
    let payload: Value = serde_json::from_slice(&body).expect("senders json");
    let row = payload
        .get("senders")
        .and_then(Value::as_array)
        .expect("senders array")
        .iter()
        .find(|row| row.get("user_id").and_then(Value::as_str) == Some(SLACK_USER))
        .cloned()
        .expect("sender present");
    assert_eq!(
        row.get("mapped"),
        Some(&json!(false)),
        "an engineering membership must not mask the sales-channel denial"
    );
    assert!(
        row.get("last_denial_reason")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .contains("do not intersect"),
        "the intersection denial reason must stay visible"
    );
    let access = row
        .get("channel_access")
        .and_then(Value::as_array)
        .cloned()
        .expect("channel_access rows");
    let by_channel = |channel: &str| {
        access
            .iter()
            .find(|entry| entry.get("channel_id").and_then(Value::as_str) == Some(channel))
            .cloned()
            .unwrap_or_else(|| panic!("channel_access row for {channel}"))
    };
    let sales = by_channel(SALES_CHANNEL);
    assert_eq!(sales.get("mapped"), Some(&json!(false)));
    assert_eq!(sales.get("bound_org_units"), Some(&json!(["sales"])));
    assert_eq!(sales.get("configured"), Some(&json!(true)));
    let eng = by_channel(ENG_CHANNEL);
    assert_eq!(eng.get("mapped"), Some(&json!(true)));
    assert_eq!(eng.get("bound_org_units"), Some(&json!(["engineering"])));
    mock_task.abort();
}

#[tokio::test]
async fn slack_verify_reports_per_connection_binding_state() {
    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    configure_slack_events(&state, &api_base_url).await;
    let app = app_router(state.clone());

    // Healthy binding: token authenticates and belongs to the configured
    // team + app.
    let request = Request::builder()
        .method("POST")
        .uri("/channels/slack/verify")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("verify request");
    let response = app.clone().oneshot(request).await.expect("verify response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("verify body");
    let payload: Value = serde_json::from_slice(&body).expect("verify json");
    assert_eq!(payload.get("ok"), Some(&json!(true)));
    let rows = payload
        .get("connections")
        .and_then(Value::as_array)
        .expect("connection rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].get("channel_id"), Some(&json!(SLACK_CHANNEL)));
    assert_eq!(rows[0].get("ok"), Some(&json!(true)));
    assert_eq!(rows[0].get("token_ok"), Some(&json!(true)));
    assert_eq!(rows[0].get("team_ok"), Some(&json!(true)));
    assert_eq!(rows[0].get("app_ok"), Some(&json!(true)));

    // Token drifting to another workspace flips the connection (and the
    // aggregate) to not-ok with an explanatory error.
    *slack_mock.auth_team_id.lock().await = "T_OTHER".to_string();
    let request = Request::builder()
        .method("POST")
        .uri("/channels/slack/verify")
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .expect("verify request");
    let response = app.oneshot(request).await.expect("drifted verify response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("drifted verify body");
    let payload: Value = serde_json::from_slice(&body).expect("drifted verify json");
    assert_eq!(payload.get("ok"), Some(&json!(false)));
    let rows = payload
        .get("connections")
        .and_then(Value::as_array)
        .expect("drifted rows");
    assert_eq!(rows[0].get("team_ok"), Some(&json!(false)));
    assert!(rows[0]
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .contains("T_OTHER"));
    mock_task.abort();
}

#[tokio::test]
async fn department_bound_connection_fails_closed_on_disjoint_membership() {
    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    // The user is an engineering member; the channel binds sales only.
    configure_slack_events_with_bound_departments(&state, &api_base_url, &["department/sales"])
        .await;
    seed_governed_slack_identity(&state).await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let request_timestamp = chrono::Utc::now().timestamp();

    let event = signed_slack_event_request(
        "Ev-dept-disjoint-1",
        SLACK_USER,
        "1800000000.420001",
        None,
        request_timestamp,
    );
    let response = app.oneshot(event).await.expect("disjoint response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        provider.calls.load(Ordering::SeqCst),
        0,
        "a disjoint membership must never dispatch a model run"
    );
    assert!(slack_mock.posts.lock().await.is_empty());

    let audit_tenant = TenantContext::explicit(ORG_ID, WORKSPACE_ID, None);
    let audit = crate::audit::load_protected_audit_events_for_tenant(&state, &audit_tenant).await;
    let denial = audit
        .iter()
        .find(|event| event.event_type == "channel.slack.ingress.denied")
        .expect("audited fail-closed denial");
    let reason = denial
        .payload
        .get("reason")
        .and_then(Value::as_str)
        .unwrap_or_default();
    assert!(
        reason.contains("department/engineering") && reason.contains("department/sales"),
        "denial must record both intersection inputs, got: {reason}"
    );
    mock_task.abort();
}

/// P1 (PR #1910 review): the HMAC must bind to the claimed installation's
/// secret. A payload claiming installation B, signed with installation A's
/// secret, must be rejected — otherwise one compromised app secret breaks
/// tenant/app isolation for every other configured installation.
#[tokio::test]
async fn slack_event_signature_must_match_the_claimed_installations_secret() {
    const SECRET_A: &str = "secret-app-a";
    const SECRET_B: &str = "secret-app-b";

    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "bot_token": "xoxb-governed-test",
                    "events_enabled": true,
                    "api_base_url": api_base_url,
                    "model_provider_id": "governed-slack-test",
                    "model_id": "governed-slack-test-1",
                    "security_profile": "trusted_team",
                    "tenant": { "org_id": ORG_ID, "workspace_id": WORKSPACE_ID },
                    "connections": [
                        {
                            "channel_id": "C_APP_A",
                            "team_id": SLACK_TEAM,
                            "app_id": "A_APP_A",
                            "signing_secret": SECRET_A,
                            "allowed_users": [SLACK_USER]
                        },
                        {
                            "channel_id": SLACK_CHANNEL,
                            "team_id": SLACK_TEAM,
                            "app_id": SLACK_APP,
                            "signing_secret": SECRET_B,
                            "allowed_users": [SLACK_USER]
                        }
                    ]
                }
            }
        }))
        .await
        .expect("configure two-app Slack connections");
    seed_governed_slack_identity(&state).await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let request_timestamp = chrono::Utc::now().timestamp();

    let event_body = |event_id: &str| {
        json!({
            "type": "event_callback",
            "event_id": event_id,
            "team_id": SLACK_TEAM,
            "api_app_id": SLACK_APP,
            "event": {
                "type": "message",
                "user": SLACK_USER,
                "channel": SLACK_CHANNEL,
                "text": "What changed for ACME?",
                "ts": "1800000000.710001"
            }
        })
        .to_string()
    };

    // Signed with the OTHER installation's secret: must fail signature
    // verification even though SECRET_A is a configured secret.
    let body = event_body("Ev-cross-secret-1");
    let forged = Request::builder()
        .method("POST")
        .uri("/channels/slack/events")
        .header("content-type", "application/json")
        .header("x-slack-request-timestamp", request_timestamp.to_string())
        .header(
            "x-slack-signature",
            super::slack_events::sign_slack_event(SECRET_A, request_timestamp, body.as_bytes()),
        )
        .body(Body::from(body))
        .expect("cross-secret request");
    let response = app
        .clone()
        .oneshot(forged)
        .await
        .expect("cross-secret response");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a payload claiming installation B must not verify against installation A's secret"
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);

    // Signed with the claimed installation's own secret: accepted end-to-end.
    let body = event_body("Ev-cross-secret-2");
    let genuine = Request::builder()
        .method("POST")
        .uri("/channels/slack/events")
        .header("content-type", "application/json")
        .header("x-slack-request-timestamp", request_timestamp.to_string())
        .header(
            "x-slack-signature",
            super::slack_events::sign_slack_event(SECRET_B, request_timestamp, body.as_bytes()),
        )
        .body(Body::from(body))
        .expect("genuine request");
    let response = app.oneshot(genuine).await.expect("genuine response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 1).await;
    wait_for_slack_tasks(&state).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    mock_task.abort();
}

/// P1 (PR #1910 review, round 3): a CONFIGURED installation that carries no
/// signing secret must fail closed — never fall back to verifying against the
/// other installations' secrets, or app A's secret could forge events for
/// app B.
#[tokio::test]
async fn slack_event_for_secretless_installation_rejects_other_apps_signatures() {
    const SECRET_A: &str = "secret-app-a";

    let state = test_state().await;
    let (api_base_url, _slack_mock, mock_task) = start_slack_api_mock().await;
    state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "bot_token": "xoxb-governed-test",
                    "events_enabled": true,
                    "api_base_url": api_base_url,
                    "model_provider_id": "governed-slack-test",
                    "model_id": "governed-slack-test-1",
                    "security_profile": "trusted_team",
                    "tenant": { "org_id": ORG_ID, "workspace_id": WORKSPACE_ID },
                    "connections": [
                        {
                            "channel_id": "C_APP_A",
                            "team_id": SLACK_TEAM,
                            "app_id": "A_APP_A",
                            "signing_secret": SECRET_A,
                            "allowed_users": [SLACK_USER]
                        },
                        {
                            // App B is configured but has NO signing secret.
                            "channel_id": SLACK_CHANNEL,
                            "team_id": SLACK_TEAM,
                            "app_id": SLACK_APP,
                            "allowed_users": [SLACK_USER]
                        }
                    ]
                }
            }
        }))
        .await
        .expect("configure secretless-app Slack connections");
    seed_governed_slack_identity(&state).await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let request_timestamp = chrono::Utc::now().timestamp();

    let body = json!({
        "type": "event_callback",
        "event_id": "Ev-secretless-1",
        "team_id": SLACK_TEAM,
        "api_app_id": SLACK_APP,
        "event": {
            "type": "message",
            "user": SLACK_USER,
            "channel": SLACK_CHANNEL,
            "text": "What changed for ACME?",
            "ts": "1800000000.910001"
        }
    })
    .to_string();
    let forged = Request::builder()
        .method("POST")
        .uri("/channels/slack/events")
        .header("content-type", "application/json")
        .header("x-slack-request-timestamp", request_timestamp.to_string())
        .header(
            "x-slack-signature",
            super::slack_events::sign_slack_event(SECRET_A, request_timestamp, body.as_bytes()),
        )
        .body(Body::from(body))
        .expect("secretless-app request");
    let response = app.oneshot(forged).await.expect("secretless-app response");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "a configured installation without its own secret must not accept another app's signature"
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    mock_task.abort();
}

/// P1 (PR #1910 review, round 3): enrollment org-unit refs must resolve in
/// the sender's tenant. With two tenants sharing a unit name, an unscoped
/// ref is rejected as ambiguous, and a tenant-scoped enrollment creates the
/// membership in exactly that tenant.
#[tokio::test]
async fn enrollment_org_units_resolve_within_the_sender_tenant() {
    use tandem_types::{OrganizationUnit, OrganizationUnitKind};

    let state = test_state().await;
    let now_ms = crate::now_ms();
    let admin = PrincipalRef::human_user("admin");
    let tenant_a = TenantContext::explicit(ORG_ID, WORKSPACE_ID, None);
    let tenant_b = TenantContext::explicit("other-org", "other-ws", None);
    // Both tenants define `department/sales`. Key the registry rows uniquely
    // (production on-disk keys are arbitrary strings, not unit ids).
    for (key, tenant) in [("a:sales", tenant_a.clone()), ("b:sales", tenant_b)] {
        let unit = OrganizationUnit::active(
            "sales",
            tenant,
            "Sales",
            OrganizationUnitKind::Department,
            admin.clone(),
            now_ms,
        )
        .with_taxonomy_id("department");
        state
            .enterprise
            .org_units
            .write()
            .await
            .insert(key.to_string(), unit);
    }
    // Tenant A also has `region/sales`, so the bare ref "sales" is ambiguous
    // even inside the tenant.
    let region = OrganizationUnit::active(
        "sales",
        tenant_a.clone(),
        "Sales Region",
        OrganizationUnitKind::Department,
        admin.clone(),
        now_ms,
    )
    .with_taxonomy_id("region");
    state
        .enterprise
        .org_units
        .write()
        .await
        .insert("a:region-sales".to_string(), region);

    let principal = format!("channel:slack:{SLACK_TEAM}:{SLACK_APP}:{SLACK_USER}");
    let tier = crate::app::state::channel_user_capabilities::StoredCommandTier::Approve;

    // Unscoped ref matching units in two tenants: rejected, not first-match.
    let ambiguous = state
        .issue_channel_enrollment_code(
            "slack",
            principal.clone(),
            tier,
            Some(60_000),
            Some("operator".to_string()),
            None,
            vec!["department/sales".to_string()],
            None,
        )
        .await;
    assert!(
        ambiguous
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default()
            .contains("ambiguous"),
        "an org unit ref matching multiple tenants must be rejected"
    );

    // Even scoped to one tenant, a bare unit id matching several taxonomies
    // (`department/sales` vs `region/sales`) is rejected, not first-match.
    let bare = state
        .issue_channel_enrollment_code(
            "slack",
            principal.clone(),
            tier,
            Some(60_000),
            Some("operator".to_string()),
            None,
            vec!["sales".to_string()],
            Some((ORG_ID.to_string(), WORKSPACE_ID.to_string())),
        )
        .await;
    assert!(
        bare.err()
            .map(|error| error.to_string())
            .unwrap_or_default()
            .contains("ambiguous within the tenant"),
        "a bare ref matching several taxonomies in one tenant must be rejected"
    );

    // Scoped to tenant A with the taxonomy-qualified id: resolves, and
    // redemption creates the membership in tenant A only.
    let code = state
        .issue_channel_enrollment_code(
            "slack",
            principal.clone(),
            tier,
            Some(60_000),
            Some("operator".to_string()),
            None,
            vec!["department/sales".to_string()],
            Some((ORG_ID.to_string(), WORKSPACE_ID.to_string())),
        )
        .await
        .expect("issue tenant-scoped enrollment code");
    assert_eq!(code.tenant_org_id.as_deref(), Some(ORG_ID));
    state
        .confirm_channel_enrollment_code(&code.code, None)
        .await
        .expect("confirm tenant-scoped enrollment code");
    let memberships = state.enterprise.org_unit_memberships.read().await;
    let created = memberships
        .values()
        .filter(|membership| membership.member.id == principal)
        .collect::<Vec<_>>();
    assert_eq!(created.len(), 1, "exactly one membership must be created");
    assert_eq!(created[0].tenant_context.org_id, ORG_ID);
    assert_eq!(created[0].tenant_context.workspace_id, WORKSPACE_ID);
}
