// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

use axum::body::{to_bytes, Body};
use axum::http::Request;
use ed25519_dalek::Signer;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tandem_types::{ApprovalDecision, ApprovalRequest, ApprovalSourceKind, ApprovalTenantRef};
use tower::ServiceExt;

const TENANT_A_ORG: &str = "org-a";
const TENANT_A_WORKSPACE: &str = "workspace-a";
const TENANT_B_ORG: &str = "org-b";
const TENANT_B_WORKSPACE: &str = "workspace-b";
const SLACK_USER: &str = "U-tenant-a";
const SLACK_TEAM: &str = "T-tenant-a";
const SLACK_APP: &str = "A-tenant-a";
const SLACK_CHANNEL: &str = "C-tenant-a";
const DISCORD_USER: &str = "discord-tenant-a";
const TELEGRAM_USER: &str = "1001";

async fn tenant_b_awaiting_run(state: &AppState) -> crate::AutomationV2RunRecord {
    let tenant_b = tandem_types::TenantContext::explicit_user_workspace(
        TENANT_B_ORG,
        TENANT_B_WORKSPACE,
        Some("deployment-b".to_string()),
        "tenant-b-actor",
    );
    let mut automation = minimal_automation("ct05-channel-routing");
    automation.set_tenant_context(&tenant_b);
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create tenant-b run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "external_action".to_string(),
                title: "Approve external action".to_string(),
                instructions: Some("approve only from the owning tenant".to_string()),
                decisions: vec!["approve".to_string(), "cancel".to_string()],
                rework_targets: Vec::new(),
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: Vec::new(),
                metadata: None,
                expiry_policy: None,
            });
        })
        .await
        .expect("mark run awaiting approval")
}

fn minimal_automation(id: &str) -> crate::AutomationV2Spec {
    crate::AutomationV2Spec {
        automation_id: id.to_string(),
        name: "CT-05 channel routing".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy::default(),
        output_targets: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        creator_id: "ct05-test".to_string(),
        workspace_root: None,
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

async fn configure_bound_channels(state: &AppState, discord_public_key: &str) {
    state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "signing_secret": "ct05-slack-secret",
                    "team_id": SLACK_TEAM,
                    "app_id": SLACK_APP,
                    "channel_id": SLACK_CHANNEL,
                    "allowed_users": [SLACK_USER],
                    "tenant": {
                        "org_id": TENANT_A_ORG,
                        "workspace_id": TENANT_A_WORKSPACE
                    }
                },
                "discord": {
                    "public_key": discord_public_key,
                    "allowed_users": [DISCORD_USER],
                    "tenant": {
                        "org_id": TENANT_A_ORG,
                        "workspace_id": TENANT_A_WORKSPACE
                    }
                },
                "telegram": {
                    "webhook_secret_token": "ct05-telegram-secret",
                    "allowed_users": [TELEGRAM_USER],
                    "tenant": {
                        "org_id": TENANT_A_ORG,
                        "workspace_id": TENANT_A_WORKSPACE
                    }
                }
            }
        }))
        .await
        .expect("patch channel config");

    for (channel, user) in [
        (
            "slack",
            format!("channel:slack:{SLACK_TEAM}:{SLACK_APP}:{SLACK_USER}"),
        ),
        ("discord", DISCORD_USER.to_string()),
        ("telegram", TELEGRAM_USER.to_string()),
    ] {
        let code = state
            .issue_channel_enrollment_code(
                channel,
                user,
                crate::app::state::channel_user_capabilities::StoredCommandTier::Approve,
                Some(60_000),
                Some("ct05-test".to_string()),
                None,
            )
            .await;
        state
            .confirm_channel_enrollment_code(&code.code, Some("ct05-test".to_string()))
            .await
            .expect("confirm channel approval capability");
    }
}

async fn seed_telegram_callback(run: &crate::AutomationV2RunRecord) {
    let map = crate::app::state::approval_message_map::ApprovalMessageMap::load_or_default(
        crate::config::paths::resolve_approval_message_map_path(),
    )
    .await;
    let request = ApprovalRequest {
        request_id: "ct05-telegram-callback".to_string(),
        approval_wait: None,
        source: ApprovalSourceKind::AutomationV2,
        tenant: ApprovalTenantRef {
            org_id: TENANT_B_ORG.to_string(),
            workspace_id: TENANT_B_WORKSPACE.to_string(),
            user_id: Some("tenant-b-actor".to_string()),
        },
        run_id: run.run_id.clone(),
        node_id: Some("external_action".to_string()),
        workflow_name: Some("CT-05 channel routing".to_string()),
        action_kind: Some("external_action".to_string()),
        action_preview_markdown: Some("test approval".to_string()),
        surface_payload: None,
        requested_at_ms: crate::now_ms(),
        expires_at_ms: None,
        decisions: vec![ApprovalDecision::Approve, ApprovalDecision::Cancel],
        rework_targets: Vec::new(),
        instructions: None,
        decided_by: None,
        decided_at_ms: None,
        decision: None,
        rework_feedback: None,
    };
    map.record_telegram_callback("tgcb_ct05", &request, TELEGRAM_USER)
        .await
        .expect("record telegram callback");
}

async fn assert_run_still_awaiting(state: &AppState, run_id: &str) {
    let run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("run remains present");
    assert_eq!(run.status, crate::AutomationRunStatus::AwaitingApproval);
    assert!(
        run.checkpoint.awaiting_gate.is_some(),
        "cross-tenant channel interaction must not decide the gate"
    );
}

fn slack_request(run_id: &str) -> Request<Body> {
    slack_request_with_nonce(run_id, 1)
}

fn slack_request_with_nonce(run_id: &str, nonce: u64) -> Request<Body> {
    let timestamp = chrono::Utc::now().timestamp();
    let payload = json!({
        "type": "block_actions",
        "api_app_id": SLACK_APP,
        "team": {"id": SLACK_TEAM},
        "channel": {"id": SLACK_CHANNEL},
        "container": {"channel_id": SLACK_CHANNEL},
        "user": {"id": SLACK_USER},
        "actions": [{
            "action_id": "approve",
            "action_ts": format!("{}.{}", timestamp, nonce),
            "value": json!({
                "correlation": {
                    "automation_v2_run_id": run_id
                }
            }).to_string()
        }]
    });
    let body = format!("payload={}", urlencoding::encode(&payload.to_string()));
    let signature = sign_slack("ct05-slack-secret", timestamp, body.as_bytes());
    Request::builder()
        .method("POST")
        .uri("/channels/slack/interactions")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-slack-request-timestamp", timestamp.to_string())
        .header("x-slack-signature", signature)
        .body(Body::from(body))
        .expect("slack request")
}

fn sign_slack(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(b"v0:");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b":");
    mac.update(body);
    format!("v0={}", hex_encode(&mac.finalize().into_bytes()))
}

fn discord_keypair() -> (ed25519_dalek::SigningKey, String) {
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&[42u8; 32]);
    let public_key = hex_encode(&signing_key.verifying_key().to_bytes());
    (signing_key, public_key)
}

fn discord_request(run_id: &str, signing_key: &ed25519_dalek::SigningKey) -> Request<Body> {
    let timestamp = "1780663300";
    let body = json!({
        "id": format!("ct05-discord-{run_id}"),
        "type": 3,
        "data": {
            "custom_id": format!("tdm:approve:{run_id}:external_action")
        },
        "member": {
            "user": {"id": DISCORD_USER}
        }
    })
    .to_string();
    let signature = sign_discord(signing_key, timestamp, body.as_bytes());
    Request::builder()
        .method("POST")
        .uri("/channels/discord/interactions")
        .header("content-type", "application/json")
        .header("x-signature-timestamp", timestamp)
        .header("x-signature-ed25519", signature)
        .body(Body::from(body))
        .expect("discord request")
}

fn sign_discord(signing_key: &ed25519_dalek::SigningKey, timestamp: &str, body: &[u8]) -> String {
    let mut signed_payload = Vec::with_capacity(timestamp.len() + body.len());
    signed_payload.extend_from_slice(timestamp.as_bytes());
    signed_payload.extend_from_slice(body);
    hex_encode(&signing_key.sign(&signed_payload).to_bytes())
}

fn telegram_request() -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/channels/telegram/interactions")
        .header("content-type", "application/json")
        .header("x-telegram-bot-api-secret-token", "ct05-telegram-secret")
        .body(Body::from(
            json!({
                "update_id": 1780663300,
                "callback_query": {
                    "id": "ct05-callback",
                    "from": {"id": TELEGRAM_USER.parse::<i64>().unwrap()},
                    "message": {"chat": {"id": 5001}},
                    "data": "tdm:approve:tgcb_ct05"
                }
            })
            .to_string(),
        ))
        .expect("telegram request")
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[tokio::test]
async fn channel_interactions_cannot_decide_run_from_other_tenant() {
    let state = test_state().await;
    let (discord_signing_key, discord_public_key) = discord_keypair();
    configure_bound_channels(&state, &discord_public_key).await;

    let run = tenant_b_awaiting_run(&state).await;
    seed_telegram_callback(&run).await;
    let app = app_router(state.clone());

    for (channel, request) in [
        ("slack", slack_request(&run.run_id)),
        (
            "discord",
            discord_request(&run.run_id, &discord_signing_key),
        ),
        ("telegram", telegram_request()),
    ] {
        let resp = app
            .clone()
            .oneshot(request)
            .await
            .unwrap_or_else(|err| panic!("{channel} response: {err}"));
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "{channel} must be tenant-bound"
        );
        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json response");
        assert_eq!(
            payload.get("reason").and_then(Value::as_str),
            Some("channel not bound to this run's tenant"),
            "{channel} denial reason"
        );
        assert_run_still_awaiting(&state, &run.run_id).await;
    }

    let audit_rows = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    let denial_events = audit_rows
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("audit json"))
        .filter(|row| {
            row.get("event_type").and_then(Value::as_str)
                == Some("channel.interaction.cross_tenant_denied")
        })
        .collect::<Vec<_>>();
    assert_eq!(denial_events.len(), 3, "one audit event per channel");
    for channel in ["slack", "discord", "telegram"] {
        let event = denial_events
            .iter()
            .find(|event| {
                event.pointer("/payload/channel").and_then(Value::as_str) == Some(channel)
            })
            .unwrap_or_else(|| panic!("missing {channel} audit event"));
        assert_eq!(
            event
                .pointer("/tenant_context/org_id")
                .and_then(Value::as_str),
            Some(TENANT_A_ORG),
            "{channel} audit attributed to the bound channel tenant"
        );
        assert_eq!(
            event
                .pointer("/tenant_context/workspace_id")
                .and_then(Value::as_str),
            Some(TENANT_A_WORKSPACE),
            "{channel} audit attributed to the bound channel workspace"
        );
        assert_eq!(
            event.pointer("/payload/run_id").and_then(Value::as_str),
            Some(run.run_id.as_str()),
            "{channel} audit run id"
        );
        assert_eq!(
            event
                .pointer("/payload/run_tenant/org_id")
                .and_then(Value::as_str),
            Some(TENANT_B_ORG),
            "{channel} audit records the denied run tenant"
        );
    }
}

async fn tenant_a_awaiting_run(state: &AppState) -> crate::AutomationV2RunRecord {
    let tenant_a = tandem_types::TenantContext::explicit_user_workspace(
        TENANT_A_ORG,
        TENANT_A_WORKSPACE,
        None,
        "tenant-a-actor",
    );
    let mut automation = minimal_automation("tan764-department-gate");
    // A real flow node backing the gate, so an authorized approve can decide
    // it (the shared minimal_automation has an empty flow and gate-decide
    // would 404 with AUTOMATION_V2_GATE_NODE_NOT_FOUND).
    automation.flow.nodes.push(crate::AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "external_action".to_string(),
        agent_id: "external_agent".to_string(),
        objective: "Perform the gated external action".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
        metadata: None,
    });
    automation.set_tenant_context(&tenant_a);
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create tenant-a run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::AwaitingApproval;
            row.checkpoint.awaiting_gate = Some(crate::AutomationPendingGate {
                node_id: "external_action".to_string(),
                title: "Approve external action".to_string(),
                instructions: None,
                decisions: vec!["approve".to_string(), "cancel".to_string()],
                rework_targets: Vec::new(),
                requested_at_ms: crate::now_ms(),
                upstream_node_ids: Vec::new(),
                metadata: None,
                expiry_policy: None,
            });
        })
        .await
        .expect("mark tenant-a run awaiting approval")
}

/// Seed a department org-unit in tenant A and make `actor_id` a member.
async fn seed_department_membership(state: &AppState, unit_id: &str, actor_id: &str) {
    use tandem_types::{
        OrganizationUnit, OrganizationUnitKind, OrganizationUnitMembership,
        OrganizationUnitMembershipSource, PrincipalRef, TenantContext,
    };
    let now_ms = crate::now_ms();
    let tenant = TenantContext::explicit(TENANT_A_ORG, TENANT_A_WORKSPACE, None);
    let admin = PrincipalRef::human_user("admin");
    let unit = OrganizationUnit::active(
        unit_id,
        tenant.clone(),
        unit_id,
        OrganizationUnitKind::Department,
        admin,
        now_ms,
    )
    .with_taxonomy_id("department");
    let actor = PrincipalRef::human_user(actor_id);
    let membership = OrganizationUnitMembership::active(
        format!("membership-{unit_id}-{actor_id}"),
        tenant,
        unit.principal_ref(),
        actor,
        OrganizationUnitMembershipSource::Direct,
        now_ms,
    );
    state
        .enterprise
        .org_units
        .write()
        .await
        .insert(unit.unit_id.clone(), unit);
    state
        .enterprise
        .org_unit_memberships
        .write()
        .await
        .insert(membership.membership_id.clone(), membership);
}

/// Re-patch the Slack channel config with a department binding (full object,
/// mirroring `configure_bound_channels`, so merge semantics cannot surprise).
async fn bind_slack_departments(state: &AppState, org_units: &[&str]) {
    state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "signing_secret": "ct05-slack-secret",
                    "team_id": SLACK_TEAM,
                    "app_id": SLACK_APP,
                    "channel_id": SLACK_CHANNEL,
                    "allowed_users": [SLACK_USER],
                    "org_units": org_units,
                    "tenant": {
                        "org_id": TENANT_A_ORG,
                        "workspace_id": TENANT_A_WORKSPACE
                    }
                }
            }
        }))
        .await
        .expect("bind slack departments");
}

#[tokio::test]
async fn slack_interaction_requires_membership_in_bound_departments() {
    let state = test_state().await;
    let (_discord_signing_key, discord_public_key) = discord_keypair();
    // Grants the Slack identity Approve capability via enrollment codes.
    configure_bound_channels(&state, &discord_public_key).await;

    // The approver is an engineering member; the channel binds sales only.
    let approver = format!("channel:slack:{SLACK_TEAM}:{SLACK_APP}:{SLACK_USER}");
    seed_department_membership(&state, "engineering", &approver).await;
    bind_slack_departments(&state, &["department/sales"]).await;

    let run = tenant_a_awaiting_run(&state).await;
    let app = app_router(state.clone());

    let resp = app
        .clone()
        .oneshot(slack_request_with_nonce(&run.run_id, 764_001))
        .await
        .expect("disjoint-department response");
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "approval authority must not exceed the channel's departmental scope"
    );
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json response");
    assert_eq!(
        payload.get("reason").and_then(Value::as_str),
        Some("user has no membership in the channel's bound departments"),
    );
    assert_run_still_awaiting(&state, &run.run_id).await;

    // Re-bind the channel to the approver's department: the same click now
    // decides the gate (regression: the gate only narrows, it does not block
    // legitimate departmental approvers).
    bind_slack_departments(&state, &["department/engineering"]).await;
    let resp = app
        .oneshot(slack_request_with_nonce(&run.run_id, 764_002))
        .await
        .expect("matching-department response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json response");
    assert_ne!(
        payload.get("ok").and_then(Value::as_bool),
        Some(false),
        "gate decision must succeed for a bound-department member: {payload}"
    );
    let run = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run present after decision");
    assert!(
        run.checkpoint.awaiting_gate.is_none(),
        "gate must be decided by the bound-department approver"
    );
}

/// Re-patch the Slack channel config with a mock API base URL so outbound
/// calls (views.open) hit the test mock instead of slack.com.
async fn point_slack_at_mock(state: &AppState, api_base_url: &str) {
    state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "signing_secret": "ct05-slack-secret",
                    "team_id": SLACK_TEAM,
                    "app_id": SLACK_APP,
                    "channel_id": SLACK_CHANNEL,
                    "allowed_users": [SLACK_USER],
                    "bot_token": "xoxb-ct05-test",
                    "api_base_url": api_base_url,
                    "tenant": {
                        "org_id": TENANT_A_ORG,
                        "workspace_id": TENANT_A_WORKSPACE
                    }
                }
            }
        }))
        .await
        .expect("point slack at mock API");
}

fn slack_rework_click_request(run_id: &str, nonce: u64) -> Request<Body> {
    let timestamp = chrono::Utc::now().timestamp();
    let payload = json!({
        "type": "block_actions",
        "api_app_id": SLACK_APP,
        "team": {"id": SLACK_TEAM},
        "channel": {"id": SLACK_CHANNEL},
        "container": {"channel_id": SLACK_CHANNEL},
        "user": {"id": SLACK_USER},
        "trigger_id": format!("trigger.{nonce}"),
        "actions": [{
            "action_id": "rework",
            "action_ts": format!("{}.{}", timestamp, nonce),
            "value": json!({
                "correlation": {
                    "automation_v2_run_id": run_id
                }
            }).to_string()
        }]
    });
    let body = format!("payload={}", urlencoding::encode(&payload.to_string()));
    let signature = sign_slack("ct05-slack-secret", timestamp, body.as_bytes());
    Request::builder()
        .method("POST")
        .uri("/channels/slack/interactions")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-slack-request-timestamp", timestamp.to_string())
        .header("x-slack-signature", signature)
        .body(Body::from(body))
        .expect("slack rework click request")
}

fn slack_view_submission_request(run_id: &str, view_id: &str, reason: &str) -> Request<Body> {
    let timestamp = chrono::Utc::now().timestamp();
    let private_metadata = json!({
        "automation_v2_run_id": run_id,
        "channel_id": SLACK_CHANNEL,
    })
    .to_string();
    let payload = json!({
        "type": "view_submission",
        "api_app_id": SLACK_APP,
        "team": {"id": SLACK_TEAM},
        "user": {"id": SLACK_USER},
        "view": {
            "id": view_id,
            "callback_id": "tandem_rework_v1",
            "private_metadata": private_metadata,
            "state": {
                "values": {
                    "reason_block": {
                        "reason_input": {
                            "type": "plain_text_input",
                            "value": reason
                        }
                    }
                }
            }
        }
    });
    let body = format!("payload={}", urlencoding::encode(&payload.to_string()));
    let signature = sign_slack("ct05-slack-secret", timestamp, body.as_bytes());
    Request::builder()
        .method("POST")
        .uri("/channels/slack/interactions")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-slack-request-timestamp", timestamp.to_string())
        .header("x-slack-signature", signature)
        .body(Body::from(body))
        .expect("slack view submission request")
}

#[tokio::test]
async fn slack_rework_modal_round_trip_dispatches_single_rework_decision() {
    let state = test_state().await;
    let (_discord_signing_key, discord_public_key) = discord_keypair();
    // Grants the Slack identity Approve capability via enrollment codes.
    configure_bound_channels(&state, &discord_public_key).await;
    let (api_base_url, slack_mock, mock_task) = super::slack_events::start_slack_api_mock().await;
    point_slack_at_mock(&state, &api_base_url).await;

    let run = tenant_a_awaiting_run(&state).await;
    let app = app_router(state.clone());

    // 1. Rework click: opens the reason modal, decides nothing.
    let resp = app
        .clone()
        .oneshot(slack_rework_click_request(&run.run_id, 767_001))
        .await
        .expect("rework click response");
    assert_eq!(resp.status(), StatusCode::OK);
    let views = slack_mock.views_opened.lock().await;
    assert_eq!(views.len(), 1, "rework click must open exactly one modal");
    assert_eq!(
        views[0].get("trigger_id").and_then(Value::as_str),
        Some("trigger.767001")
    );
    assert_eq!(
        views[0]
            .pointer("/view/callback_id")
            .and_then(Value::as_str),
        Some("tandem_rework_v1")
    );
    let metadata: Value = views[0]
        .pointer("/view/private_metadata")
        .and_then(Value::as_str)
        .and_then(|raw| serde_json::from_str(raw).ok())
        .expect("modal private_metadata");
    assert_eq!(
        metadata.get("automation_v2_run_id").and_then(Value::as_str),
        Some(run.run_id.as_str())
    );
    assert_eq!(
        metadata.get("channel_id").and_then(Value::as_str),
        Some(SLACK_CHANNEL)
    );
    drop(views);
    assert_run_still_awaiting(&state, &run.run_id).await;

    // 2. Submitting an empty reason returns Slack's inline-validation shape
    //    and decides nothing.
    let resp = app
        .clone()
        .oneshot(slack_view_submission_request(
            &run.run_id,
            "V767_EMPTY",
            "   ",
        ))
        .await
        .expect("empty-reason response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json response");
    assert_eq!(
        payload.get("response_action").and_then(Value::as_str),
        Some("errors"),
        "empty reason must surface inline modal validation"
    );
    assert_run_still_awaiting(&state, &run.run_id).await;

    // 3. A real submission dispatches exactly one rework decision with the
    //    collected reason.
    let resp = app
        .clone()
        .oneshot(slack_view_submission_request(
            &run.run_id,
            "V767_REAL",
            "Tighten the executive summary before resending.",
        ))
        .await
        .expect("rework submission response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json response");
    assert!(
        payload.get("response_action").is_none(),
        "successful submission must close the modal, got: {payload}"
    );
    let decided = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run present after rework");
    assert!(
        decided.checkpoint.awaiting_gate.is_none(),
        "rework submission must decide the gate"
    );

    // 4. A duplicate submission (Slack double-fire) is dropped by the view-id
    //    dedup and never double-decides.
    let resp = app
        .oneshot(slack_view_submission_request(
            &run.run_id,
            "V767_REAL",
            "Tighten the executive summary before resending.",
        ))
        .await
        .expect("duplicate submission response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json response");
    assert!(
        payload.get("response_action").is_none(),
        "duplicate must be acked as a silent no-op, got: {payload}"
    );
    mock_task.abort();
}
