// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

use async_trait::async_trait;
use axum::{
    extract::State,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::{stream, Stream};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tandem_providers::{ChatMessage, Provider, StreamChunk};
use tandem_types::{
    AccessPermission, DataClass, ModelInfo, OrganizationUnit, OrganizationUnitAccessGrant,
    OrganizationUnitKind, OrganizationUnitMembership, OrganizationUnitMembershipSource,
    PrincipalRef, ProviderInfo, ResourceKind, ResourceRef, TenantContext, ToolMode, ToolSchema,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const SIGNING_SECRET: &str = "slack-events-test-secret";
const SLACK_USER: &str = "U_GOVERNED";
const SLACK_CHANNEL: &str = "C_GOVERNED";
const SLACK_TEAM: &str = "T_GOVERNED";
const SLACK_APP: &str = "A_GOVERNED";
const ORG_ID: &str = "acme";
const WORKSPACE_ID: &str = "hq";

#[derive(Clone, Default)]
struct GovernedSlackProbe {
    calls: Arc<AtomicUsize>,
    tools_seen: Arc<Mutex<Vec<Vec<String>>>>,
    prompts_seen: Arc<Mutex<Vec<Vec<String>>>>,
    failures_remaining: Arc<AtomicUsize>,
    block_until_cancel: Arc<AtomicBool>,
}

struct GovernedSlackProvider {
    probe: GovernedSlackProbe,
}

#[async_trait]
impl Provider for GovernedSlackProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "governed-slack-test".to_string(),
            name: "Governed Slack Test".to_string(),
            models: vec![ModelInfo {
                id: "governed-slack-test-1".to_string(),
                provider_id: "governed-slack-test".to_string(),
                display_name: "Governed Slack Test 1".to_string(),
                context_window: 8_192,
            }],
        }
    }

    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        Ok("governed reply".to_string())
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        _sampling: tandem_types::SamplingParams,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        self.probe.calls.fetch_add(1, Ordering::SeqCst);
        self.probe.prompts_seen.lock().await.push(
            messages
                .into_iter()
                .map(|message| message.content)
                .collect(),
        );
        let mut tool_names = tools
            .unwrap_or_default()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        tool_names.sort();
        self.probe.tools_seen.lock().await.push(tool_names);
        if self
            .probe
            .failures_remaining
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
                (remaining > 0).then(|| remaining - 1)
            })
            .is_ok()
        {
            anyhow::bail!("governed Slack provider failed for test");
        }
        if self.probe.block_until_cancel.load(Ordering::SeqCst) {
            cancel.cancelled().await;
            anyhow::bail!("governed Slack provider cancelled for test");
        }
        Ok(Box::pin(stream::iter(vec![
            Ok(StreamChunk::TextDelta("governed reply".to_string())),
            Ok(StreamChunk::Done {
                finish_reason: "stop".to_string(),
                usage: None,
            }),
        ])))
    }
}

async fn install_governed_slack_provider(state: &AppState, failures: usize) -> GovernedSlackProbe {
    let probe = GovernedSlackProbe {
        failures_remaining: Arc::new(AtomicUsize::new(failures)),
        ..Default::default()
    };
    state
        .providers
        .replace_for_test(
            vec![Arc::new(GovernedSlackProvider {
                probe: probe.clone(),
            })],
            Some("governed-slack-test".to_string()),
        )
        .await;
    probe
}

struct SlackVisibleTool;

#[async_trait]
impl tandem_tools::Tool for SlackVisibleTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "slack.visible",
            "A tool that must not leak through an empty governed allowlist",
            json!({ "type": "object" }),
        )
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<tandem_types::ToolResult> {
        Ok(tandem_types::ToolResult {
            output: "visible".to_string(),
            metadata: json!({}),
        })
    }
}

#[derive(Clone, Default)]
pub(super) struct SlackApiMock {
    pub(super) posts: Arc<Mutex<Vec<Value>>>,
    attempts: Arc<AtomicUsize>,
    auth_attempts: Arc<AtomicUsize>,
    bots_info_attempts: Arc<AtomicUsize>,
    pub(super) auth_team_id: Arc<Mutex<String>>,
    pub(super) auth_app_id: Arc<Mutex<String>>,
    auth_is_bot: Arc<AtomicBool>,
    failures_remaining: Arc<AtomicUsize>,
}

async fn slack_bots_info(
    State(state): State<SlackApiMock>,
    axum::extract::Query(query): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    state.bots_info_attempts.fetch_add(1, Ordering::SeqCst);
    let bot_id = query.get("bot").cloned().unwrap_or_default();
    let app_id = state.auth_app_id.lock().await.clone();
    Json(json!({
        "ok": true,
        "bot": {
            "id": bot_id,
            "app_id": app_id,
        }
    }))
    .into_response()
}

async fn slack_auth_test(State(state): State<SlackApiMock>) -> Response {
    state.auth_attempts.fetch_add(1, Ordering::SeqCst);
    let team_id = state.auth_team_id.lock().await.clone();
    let mut payload = json!({
        "ok": true,
        "team_id": team_id,
        "user_id": "U_BOT"
    });
    if state.auth_is_bot.load(Ordering::SeqCst) {
        payload["bot_id"] = json!("B_GOVERNED");
    }
    Json(payload).into_response()
}

async fn slack_post_message(
    State(state): State<SlackApiMock>,
    Json(payload): Json<Value>,
) -> Response {
    state.attempts.fetch_add(1, Ordering::SeqCst);
    if state
        .failures_remaining
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
            (remaining > 0).then(|| remaining - 1)
        })
        .is_ok()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "ok": false, "error": "transient_failure" })),
        )
            .into_response();
    }
    state.posts.lock().await.push(payload.clone());
    Json(json!({
        "ok": true,
        "channel": payload.get("channel").and_then(Value::as_str).unwrap_or(SLACK_CHANNEL),
        "ts": "1800000000.000001"
    }))
    .into_response()
}

pub(super) async fn start_slack_api_mock() -> (String, SlackApiMock, tokio::task::JoinHandle<()>) {
    start_slack_api_mock_with_failures(0).await
}

async fn start_slack_api_mock_with_failures(
    failures: usize,
) -> (String, SlackApiMock, tokio::task::JoinHandle<()>) {
    let state = SlackApiMock {
        failures_remaining: Arc::new(AtomicUsize::new(failures)),
        auth_team_id: Arc::new(Mutex::new(SLACK_TEAM.to_string())),
        auth_app_id: Arc::new(Mutex::new(SLACK_APP.to_string())),
        auth_is_bot: Arc::new(AtomicBool::new(true)),
        ..Default::default()
    };
    let app = Router::new()
        .route("/auth.test", get(slack_auth_test))
        .route("/bots.info", get(slack_bots_info))
        .route("/chat.postMessage", post(slack_post_message))
        .with_state(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind Slack API mock");
    let address = listener.local_addr().expect("Slack API mock address");
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve Slack API mock");
    });
    (format!("http://{address}"), state, task)
}

async fn configure_slack_events(state: &AppState, api_base_url: &str) {
    configure_slack_events_for_installation(
        state,
        api_base_url,
        SLACK_TEAM,
        SLACK_APP,
        SLACK_CHANNEL,
        &[SLACK_USER],
    )
    .await;
}

pub(super) async fn configure_slack_events_for_installation(
    state: &AppState,
    api_base_url: &str,
    team_id: &str,
    app_id: &str,
    channel_id: &str,
    allowed_users: &[&str],
) {
    state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "signing_secret": SIGNING_SECRET,
                    "events_enabled": true,
                    "bot_token": "xoxb-governed-test",
                    "channel_id": channel_id,
                    "team_id": team_id,
                    "app_id": app_id,
                    "allowed_users": allowed_users,
                    "api_base_url": api_base_url,
                    "model_provider_id": "governed-slack-test",
                    "model_id": "governed-slack-test-1",
                    "security_profile": "trusted_team",
                    "tenant": {
                        "org_id": ORG_ID,
                        "workspace_id": WORKSPACE_ID
                    }
                }
            }
        }))
        .await
        .expect("configure Slack Events");
}

async fn seed_governed_slack_identity(state: &AppState) {
    seed_governed_slack_identity_with_tools(state, &["mcp.github.*"]).await;
}

async fn seed_governed_slack_identity_with_tools(state: &AppState, tool_patterns: &[&str]) {
    seed_governed_slack_identity_for_user(state, SLACK_USER, tool_patterns).await;
}

async fn seed_governed_slack_identity_for_user(
    state: &AppState,
    user_id: &str,
    tool_patterns: &[&str],
) {
    let now_ms = crate::now_ms();
    let tenant = TenantContext::explicit(ORG_ID, WORKSPACE_ID, None);
    let admin = PrincipalRef::human_user("admin");
    let department = OrganizationUnit::active(
        "engineering",
        tenant.clone(),
        "Engineering",
        OrganizationUnitKind::Department,
        admin.clone(),
        now_ms,
    )
    .with_taxonomy_id("department");
    let role = OrganizationUnit::active(
        "engineer",
        tenant.clone(),
        "Engineer",
        OrganizationUnitKind::RoleDomain,
        admin,
        now_ms,
    )
    .with_taxonomy_id("role");
    let actor =
        PrincipalRef::human_user(format!("channel:slack:{SLACK_TEAM}:{SLACK_APP}:{user_id}"));
    let memberships = [
        OrganizationUnitMembership::active(
            format!("membership-engineering-{user_id}"),
            tenant.clone(),
            department.principal_ref(),
            actor.clone(),
            OrganizationUnitMembershipSource::Direct,
            now_ms,
        ),
        OrganizationUnitMembership::active(
            format!("membership-engineer-role-{user_id}"),
            tenant.clone(),
            role.principal_ref(),
            actor,
            OrganizationUnitMembershipSource::Direct,
            now_ms,
        ),
    ];
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

    state.enterprise.org_units.write().await.extend([
        (department.unit_id.clone(), department),
        (role.unit_id.clone(), role),
    ]);
    state.enterprise.org_unit_memberships.write().await.extend(
        memberships
            .into_iter()
            .map(|row| (row.membership_id.clone(), row)),
    );
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert(grant.grant_id.clone(), grant);
}

pub(super) async fn seed_acme_demo_authority(
    state: &AppState,
) -> crate::acme_demo::AcmeDemoDataset {
    let dataset = crate::acme_demo::acme_demo_dataset();
    state.enterprise.org_units.write().await.extend(
        dataset
            .graph
            .units
            .iter()
            .cloned()
            .map(|unit| (unit.unit_id.clone(), unit)),
    );
    state.enterprise.org_unit_memberships.write().await.extend(
        dataset
            .graph
            .memberships
            .iter()
            .cloned()
            .map(|membership| (membership.membership_id.clone(), membership)),
    );
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .extend(
            dataset
                .graph
                .unit_access_grants
                .iter()
                .cloned()
                .map(|grant| (grant.grant_id.clone(), grant)),
        );
    dataset
}

fn signed_slack_event_request(
    event_id: &str,
    user_id: &str,
    message_ts: &str,
    thread_ts: Option<&str>,
    request_timestamp: i64,
) -> Request<Body> {
    signed_slack_event_request_for_installation(
        event_id,
        user_id,
        SLACK_CHANNEL,
        Some(SLACK_TEAM),
        Some(SLACK_APP),
        message_ts,
        thread_ts,
        request_timestamp,
    )
}

pub(super) fn signed_slack_event_request_for_installation(
    event_id: &str,
    user_id: &str,
    channel_id: &str,
    team_id: Option<&str>,
    app_id: Option<&str>,
    message_ts: &str,
    thread_ts: Option<&str>,
    request_timestamp: i64,
) -> Request<Body> {
    signed_slack_event_request_with_text(
        event_id,
        user_id,
        channel_id,
        team_id,
        app_id,
        message_ts,
        thread_ts,
        request_timestamp,
        "What changed for ACME?",
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn signed_slack_event_request_with_text(
    event_id: &str,
    user_id: &str,
    channel_id: &str,
    team_id: Option<&str>,
    app_id: Option<&str>,
    message_ts: &str,
    thread_ts: Option<&str>,
    request_timestamp: i64,
    text: &str,
) -> Request<Body> {
    let mut event = json!({
        "type": "message",
        "user": user_id,
        "channel": channel_id,
        "text": text,
        "ts": message_ts
    });
    if let Some(thread_ts) = thread_ts {
        event["thread_ts"] = json!(thread_ts);
    }
    let mut payload = json!({
        "type": "event_callback",
        "event_id": event_id,
        "event": event
    });
    if let Some(team_id) = team_id {
        payload["team_id"] = json!(team_id);
    }
    if let Some(app_id) = app_id {
        payload["api_app_id"] = json!(app_id);
    }
    let body = payload.to_string();
    let signature = sign_slack_event(SIGNING_SECRET, request_timestamp, body.as_bytes());
    Request::builder()
        .method("POST")
        .uri("/channels/slack/events")
        .header("content-type", "application/json")
        .header("x-slack-request-timestamp", request_timestamp.to_string())
        .header("x-slack-signature", signature)
        .body(Body::from(body))
        .expect("Slack event request")
}

fn signed_slack_url_verification_request(challenge: &str, request_timestamp: i64) -> Request<Body> {
    let body = json!({
        "token": "legacy-verification-token",
        "challenge": challenge,
        "type": "url_verification"
    })
    .to_string();
    let signature = sign_slack_event(SIGNING_SECRET, request_timestamp, body.as_bytes());
    Request::builder()
        .method("POST")
        .uri("/channels/slack/events")
        .header("content-type", "application/json")
        .header("x-slack-request-timestamp", request_timestamp.to_string())
        .header("x-slack-signature", signature)
        .body(Body::from(body))
        .expect("Slack URL verification request")
}

fn sign_slack_event(secret: &str, timestamp: i64, body: &[u8]) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).expect("HMAC key");
    mac.update(b"v0:");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b":");
    mac.update(body);
    format!("v0={}", hex_encode(&mac.finalize().into_bytes()))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn slack_private_memory_record(
    id: &str,
    subject: &str,
    content: &str,
) -> tandem_memory::types::GlobalMemoryRecord {
    tandem_memory::types::GlobalMemoryRecord {
        id: id.to_string(),
        user_id: subject.to_string(),
        source_type: "channel_message".to_string(),
        content: content.to_string(),
        content_hash: format!("hash-{id}"),
        run_id: format!("run-{id}"),
        session_id: None,
        message_id: Some(id.to_string()),
        tool_name: None,
        project_tag: None,
        channel_tag: Some(SLACK_CHANNEL.to_string()),
        host_tag: None,
        metadata: Some(json!({ "owner_subject": subject })),
        provenance: Some(json!({
            "tenant_context": {
                "org_id": ORG_ID,
                "workspace_id": WORKSPACE_ID,
                "deployment_id": null
            }
        })),
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "private".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        expires_at_ms: None,
    }
}

pub(super) async fn wait_for_posts(mock: &SlackApiMock, expected: usize) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if mock.posts.lock().await.len() >= expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Slack post timeout");
}

async fn wait_for_counter(counter: &AtomicUsize, expected: usize, label: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if counter.load(Ordering::SeqCst) >= expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{label} timeout"));
}

pub(super) async fn wait_for_slack_tasks(state: &AppState) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if state.slack_event_tasks.active_count().await == 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("Slack task drain timeout");
}

#[tokio::test]
async fn slack_capability_and_step_up_do_not_cross_installations() {
    let state = test_state().await;
    let first = "channel:slack:T1:A1:U1";
    let second = "channel:slack:T1:A2:U1";
    state
        .upsert_channel_user_capability(
            crate::app::state::channel_user_capabilities::ChannelUserCapabilityRecord {
                channel: "slack".to_string(),
                user_id: first.to_string(),
                max_tier: crate::app::state::channel_user_capabilities::StoredCommandTier::Approve,
                enrolled_at_ms: Some(crate::now_ms()),
                enrolled_by: Some("test".to_string()),
                pinned_workspace_id: None,
            },
        )
        .await
        .expect("grant first Slack installation capability");
    assert!(
        state
            .channel_user_can_approve(
                "slack",
                first,
                tandem_channels::config::ChannelSecurityProfile::PublicDemo,
                true,
            )
            .await
    );
    assert!(
        !state
            .channel_user_can_approve(
                "slack",
                second,
                tandem_channels::config::ChannelSecurityProfile::PublicDemo,
                true,
            )
            .await
    );
    state.grant_channel_step_up("slack", first, 60_000).await;
    assert!(state.channel_step_up_active("slack", first).await);
    assert!(!state.channel_step_up_active("slack", second).await);
}

#[tokio::test]
async fn signed_slack_events_run_with_governed_context_reuse_and_dedupe() {
    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    configure_slack_events(&state, &api_base_url).await;
    seed_governed_slack_identity(&state).await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let request_timestamp = chrono::Utc::now().timestamp();
    let root_ts = "1800000000.100001";

    let first = signed_slack_event_request(
        "Ev-governed-1",
        SLACK_USER,
        root_ts,
        None,
        request_timestamp,
    );
    let response = app.clone().oneshot(first).await.expect("first response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 1).await;
    wait_for_slack_tasks(&state).await;

    let duplicate = signed_slack_event_request(
        "Ev-governed-1",
        SLACK_USER,
        root_ts,
        None,
        request_timestamp,
    );
    let response = app
        .clone()
        .oneshot(duplicate)
        .await
        .expect("retry response");
    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(slack_mock.posts.lock().await.len(), 1);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    let second = signed_slack_event_request(
        "Ev-governed-2",
        SLACK_USER,
        "1800000000.200001",
        Some(root_ts),
        request_timestamp,
    );
    let response = app.oneshot(second).await.expect("thread response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 2).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    wait_for_slack_tasks(&state).await;

    let sessions = state.storage.list_sessions().await;
    assert_eq!(sessions.len(), 1, "thread events must reuse one session");
    let session = &sessions[0];
    assert_eq!(session.tenant_context.org_id, ORG_ID);
    assert_eq!(session.tenant_context.workspace_id, WORKSPACE_ID);
    assert_eq!(
        session.tenant_context.actor_id.as_deref(),
        Some("channel:slack:T_GOVERNED:A_GOVERNED:U_GOVERNED")
    );
    let verified = session
        .verified_tenant_context
        .as_ref()
        .expect("verified channel context");
    assert_eq!(
        verified.human_actor.subject.as_deref(),
        Some("channel:slack:T_GOVERNED:A_GOVERNED:U_GOVERNED")
    );
    assert_eq!(
        verified.org_units,
        vec!["department/engineering", "role/engineer"]
    );
    assert_eq!(verified.roles, vec!["engineer"]);
    assert_eq!(verified.capabilities, vec!["mcp.github.*"]);
    let strict = verified
        .strict_projection
        .as_ref()
        .expect("strict projection");
    assert_eq!(strict.grants.len(), 1);
    assert_eq!(
        strict.grants[0].grant_id,
        "department/engineering::engineering-read"
    );
    assert!(strict.grants[0]
        .permissions
        .contains(&AccessPermission::Execute));
    assert!(strict.allows_data_class(DataClass::Internal));
    assert!(!strict.allows_data_class(DataClass::Credential));
    assert_eq!(
        session
            .source_metadata
            .as_ref()
            .and_then(|value| value.get("scope_id"))
            .and_then(Value::as_str),
        Some("thread:T_GOVERNED:A_GOVERNED:C_GOVERNED:1800000000.100001")
    );
    let posts = slack_mock.posts.lock().await;
    assert_eq!(posts[0]["channel"], SLACK_CHANNEL);
    assert_eq!(posts[0]["thread_ts"], root_ts);
    assert_eq!(posts[0]["text"], "governed reply");
    assert_eq!(posts[1]["thread_ts"], root_ts);
    drop(posts);

    let audit_tenant = TenantContext::explicit(ORG_ID, WORKSPACE_ID, None);
    let audit = crate::audit::load_protected_audit_events_for_tenant(&state, &audit_tenant).await;
    for event_type in [
        "channel.slack.ingress.accepted",
        "channel.slack.run.started",
        "channel.slack.run.completed",
        "channel.slack.response.delivered",
    ] {
        assert!(
            audit.iter().any(|event| event.event_type == event_type),
            "missing Slack audit event {event_type}"
        );
    }
    assert!(
        audit
            .iter()
            .filter(|event| {
                event.event_type == "channel.slack.run.started"
                    && event.payload.pointer("/dimensions/slack_team_id")
                        == Some(&json!(SLACK_TEAM))
                    && event.payload.pointer("/dimensions/slack_app_id") == Some(&json!(SLACK_APP))
            })
            .count()
            >= 2
    );
    mock_task.abort();
}

#[tokio::test]
async fn signed_slack_senders_receive_only_their_private_prompt_memory() {
    const ALICE: &str = "U_ALICE";
    const BOB: &str = "U_BOB";
    const ALICE_MARKER: &str = "meteor-alice-private-marker";
    const BOB_MARKER: &str = "meteor-bob-forbidden-marker";

    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    configure_slack_events_for_installation(
        &state,
        &api_base_url,
        SLACK_TEAM,
        SLACK_APP,
        SLACK_CHANNEL,
        &[ALICE, BOB],
    )
    .await;
    seed_governed_slack_identity_for_user(&state, ALICE, &["memory.*"]).await;
    seed_governed_slack_identity_for_user(&state, BOB, &["memory.*"]).await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let database = tandem_memory::db::MemoryDatabase::new(&state.memory_db_path)
        .await
        .expect("memory database");
    let alice_subject = format!("channel:slack:{SLACK_TEAM}:{SLACK_APP}:{ALICE}");
    let bob_subject = format!("channel:slack:{SLACK_TEAM}:{SLACK_APP}:{BOB}");
    database
        .put_global_memory_record(&slack_private_memory_record(
            "slack-alice-private",
            &alice_subject,
            &format!("What changed for ACME? {ALICE_MARKER}"),
        ))
        .await
        .expect("store Alice memory");
    database
        .put_global_memory_record(&slack_private_memory_record(
            "slack-bob-private",
            &bob_subject,
            &format!("What changed for ACME? {BOB_MARKER}"),
        ))
        .await
        .expect("store Bob memory");

    let app = app_router(state.clone());
    let timestamp = chrono::Utc::now().timestamp();
    let alice_response = app
        .clone()
        .oneshot(signed_slack_event_request(
            "Ev-private-alice",
            ALICE,
            "1800000100.100001",
            None,
            timestamp,
        ))
        .await
        .expect("Alice signed event");
    assert_eq!(alice_response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 1).await;
    wait_for_slack_tasks(&state).await;

    let bob_response = app
        .oneshot(signed_slack_event_request(
            "Ev-private-bob",
            BOB,
            "1800000100.200001",
            None,
            timestamp,
        ))
        .await
        .expect("Bob signed event");
    assert_eq!(bob_response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 2).await;
    wait_for_slack_tasks(&state).await;

    let prompts = provider.prompts_seen.lock().await;
    assert_eq!(prompts.len(), 2);
    let alice_prompt = prompts[0].join("\n");
    let bob_prompt = prompts[1].join("\n");
    assert!(alice_prompt.contains(ALICE_MARKER), "{alice_prompt}");
    assert!(!alice_prompt.contains(BOB_MARKER), "{alice_prompt}");
    assert!(bob_prompt.contains(BOB_MARKER), "{bob_prompt}");
    assert!(!bob_prompt.contains(ALICE_MARKER), "{bob_prompt}");

    let tools = provider.tools_seen.lock().await;
    assert_eq!(tools.len(), 2);
    assert!(tools.iter().all(|names| {
        names
            .iter()
            .all(|name| name.starts_with("memory.") || name.starts_with("mcp.memory."))
    }));
    mock_task.abort();
}

#[tokio::test]
async fn slack_events_reject_invalid_and_expired_signatures_with_forbidden() {
    let state = test_state().await;
    configure_slack_events(&state, "http://127.0.0.1:9").await;
    let app = app_router(state.clone());
    let now = chrono::Utc::now().timestamp();

    let mut invalid = signed_slack_event_request("Ev-invalid", SLACK_USER, "1.0", None, now);
    invalid
        .headers_mut()
        .insert("x-slack-signature", "v0=invalid".parse().unwrap());
    let response = app
        .clone()
        .oneshot(invalid)
        .await
        .expect("invalid response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let expired = signed_slack_event_request("Ev-expired", SLACK_USER, "2.0", None, now - 301);
    let response = app.oneshot(expired).await.expect("expired response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let audit = crate::audit::load_protected_audit_events_for_tenant(
        &state,
        &TenantContext::explicit(ORG_ID, WORKSPACE_ID, None),
    )
    .await;
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.event_type == "channel.slack.ingress.denied")
            .count(),
        2
    );
}

#[tokio::test]
async fn slack_events_accept_standard_signed_url_verification_payload() {
    let state = test_state().await;
    configure_slack_events(&state, "http://127.0.0.1:9").await;
    let app = app_router(state);
    let now = chrono::Utc::now().timestamp();

    let response = app
        .clone()
        .oneshot(signed_slack_url_verification_request(
            "standard-slack-challenge",
            now,
        ))
        .await
        .expect("URL verification response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("URL verification body");
    assert_eq!(body.as_ref(), b"standard-slack-challenge");

    let mut forged = signed_slack_url_verification_request("forged-challenge", now);
    forged
        .headers_mut()
        .insert("x-slack-signature", "v0=invalid".parse().unwrap());
    let response = app
        .oneshot(forged)
        .await
        .expect("forged URL verification response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn slack_events_reject_unauthorized_or_unmapped_senders_without_dispatch() {
    let state = test_state().await;
    configure_slack_events(&state, "http://127.0.0.1:9").await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let unmapped = signed_slack_event_request(
        "Ev-unmapped-governed-identity",
        SLACK_USER,
        "1800000000.300001",
        None,
        chrono::Utc::now().timestamp(),
    );
    let response = app
        .clone()
        .oneshot(unmapped)
        .await
        .expect("unmapped governed identity response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    let unauthorized = signed_slack_event_request(
        "Ev-not-allowlisted",
        "U_UNMAPPED",
        "1800000000.400001",
        None,
        chrono::Utc::now().timestamp(),
    );
    let response = app
        .oneshot(unauthorized)
        .await
        .expect("unauthorized response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert!(state.storage.list_sessions().await.is_empty());
}

#[tokio::test]
async fn slack_events_fail_closed_on_missing_or_mismatched_installation() {
    let state = test_state().await;
    configure_slack_events(&state, "http://127.0.0.1:9").await;
    seed_governed_slack_identity(&state).await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let now = chrono::Utc::now().timestamp();
    let cases = [
        ("Ev-wrong-team", Some("T_OTHER"), Some(SLACK_APP)),
        ("Ev-missing-team", None, Some(SLACK_APP)),
        ("Ev-wrong-app", Some(SLACK_TEAM), Some("A_OTHER")),
        ("Ev-missing-app", Some(SLACK_TEAM), None),
    ];

    for (index, (event_id, team_id, app_id)) in cases.into_iter().enumerate() {
        let request = signed_slack_event_request_for_installation(
            event_id,
            SLACK_USER,
            SLACK_CHANNEL,
            team_id,
            app_id,
            &format!("1800000001.{index:06}"),
            None,
            now,
        );
        let response = app
            .clone()
            .oneshot(request)
            .await
            .expect("installation rejection response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{event_id}");
    }

    assert_eq!(provider.calls.load(Ordering::SeqCst), 0);
    assert!(state.storage.list_sessions().await.is_empty());
    let tenant = TenantContext::explicit(ORG_ID, WORKSPACE_ID, None);
    let audit = crate::audit::load_protected_audit_events_for_tenant(&state, &tenant).await;
    assert_eq!(
        audit
            .iter()
            .filter(|event| event.event_type == "channel.slack.ingress.denied")
            .count(),
        4
    );
    let unrelated = TenantContext::explicit("other-org", WORKSPACE_ID, None);
    assert!(
        crate::audit::load_protected_audit_events_for_tenant(&state, &unrelated)
            .await
            .is_empty(),
        "Slack denial audits must not cross tenant boundaries"
    );
}

#[tokio::test]
async fn all_five_acme_profiles_enter_through_strict_governed_slack_ingress() {
    use crate::acme_demo::harness::DEMO_SLACK_CHANNEL_ID;
    use crate::acme_demo::{DEMO_SLACK_APP_ID, DEMO_SLACK_TEAM_ID};

    let state = test_state().await;
    let dataset = seed_acme_demo_authority(&state).await;
    let allowed_users = dataset
        .profiles
        .iter()
        .map(|profile| profile.slack_user_id)
        .collect::<Vec<_>>();
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    *slack_mock.auth_team_id.lock().await = DEMO_SLACK_TEAM_ID.to_string();
    *slack_mock.auth_app_id.lock().await = DEMO_SLACK_APP_ID.to_string();
    configure_slack_events_for_installation(
        &state,
        &api_base_url,
        DEMO_SLACK_TEAM_ID,
        DEMO_SLACK_APP_ID,
        DEMO_SLACK_CHANNEL_ID,
        &allowed_users,
    )
    .await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let app = app_router(state.clone());
    let now = chrono::Utc::now().timestamp();

    for (index, profile) in dataset.profiles.iter().enumerate() {
        let response = app
            .clone()
            .oneshot(signed_slack_event_request_for_installation(
                &format!("Ev-acme-profile-{index}"),
                profile.slack_user_id,
                DEMO_SLACK_CHANNEL_ID,
                Some(DEMO_SLACK_TEAM_ID),
                Some(DEMO_SLACK_APP_ID),
                &format!("1800000010.{index:06}"),
                None,
                now,
            ))
            .await
            .expect("ACME profile Slack response");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "profile {}",
            profile.slack_user_id
        );
    }

    wait_for_posts(&slack_mock, dataset.profiles.len()).await;
    wait_for_slack_tasks(&state).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 5);
    let sessions = state.storage.list_sessions().await;
    assert_eq!(sessions.len(), 5);

    for profile in &dataset.profiles {
        let session = sessions
            .iter()
            .find(|session| session.tenant_context.actor_id.as_deref() == Some(&profile.actor_id))
            .unwrap_or_else(|| panic!("missing session for {}", profile.slack_user_id));
        let verified = session
            .verified_tenant_context
            .as_ref()
            .expect("ACME verified context");
        let expected_subject = format!(
            "channel:slack:{DEMO_SLACK_TEAM_ID}:{DEMO_SLACK_APP_ID}:{}",
            profile.slack_user_id
        );
        assert_eq!(
            verified.human_actor.subject.as_deref(),
            Some(expected_subject.as_str())
        );
        assert_eq!(verified.org_units, profile.org_units());
        assert_eq!(verified.issuer, "tandem-server:slack-events");
        assert_eq!(verified.audience, "tandem-engine");

        let strict = verified
            .strict_projection
            .as_ref()
            .expect("ACME strict authority projection");
        assert_eq!(strict.principal, profile.principal);
        assert_eq!(strict.tenant_context.org_id, crate::acme_demo::DEMO_ORG_ID);
        assert_eq!(
            strict.tenant_context.workspace_id,
            crate::acme_demo::DEMO_WORKSPACE_ID
        );
        let expected_grants = dataset
            .graph
            .effective_grants(&profile.principal, crate::now_ms());
        let mut expected_grant_ids = expected_grants
            .iter()
            .map(|grant| grant.grant_id.clone())
            .collect::<Vec<_>>();
        expected_grant_ids.sort();
        let actual_grant_ids = strict
            .grants
            .iter()
            .map(|grant| grant.grant_id.clone())
            .collect::<Vec<_>>();
        assert_eq!(actual_grant_ids, expected_grant_ids);

        let mut expected_capabilities = expected_grants
            .iter()
            .filter(|grant| grant.effect == tandem_types::AccessEffect::Allow)
            .flat_map(|grant| grant.tool_patterns.iter().cloned())
            .collect::<Vec<_>>();
        expected_capabilities.sort();
        expected_capabilities.dedup();
        assert_eq!(verified.capabilities, expected_capabilities);
    }
    mock_task.abort();
}

#[tokio::test]
async fn slack_principal_without_tool_patterns_receives_no_tool_authority() {
    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    configure_slack_events(&state, &api_base_url).await;
    seed_governed_slack_identity_with_tools(&state, &[]).await;
    state
        .tools
        .register_tool("slack.visible".to_string(), Arc::new(SlackVisibleTool))
        .await;
    let provider = install_governed_slack_provider(&state, 0).await;
    let response = app_router(state.clone())
        .oneshot(signed_slack_event_request(
            "Ev-empty-tool-authority",
            SLACK_USER,
            "1800000020.000001",
            None,
            chrono::Utc::now().timestamp(),
        ))
        .await
        .expect("empty tool authority response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 1).await;
    wait_for_slack_tasks(&state).await;

    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        provider.tools_seen.lock().await.as_slice(),
        &[Vec::<String>::new()]
    );
    let sessions = state.storage.list_sessions().await;
    let verified = sessions[0]
        .verified_tenant_context
        .as_ref()
        .expect("verified context");
    assert!(verified.capabilities.is_empty());
    assert!(verified
        .strict_projection
        .as_ref()
        .expect("strict context")
        .grants
        .iter()
        .all(|grant| grant.tool_patterns.is_empty()));
    mock_task.abort();
}

#[tokio::test]
async fn staged_response_replays_after_restart_and_completed_duplicate_is_suppressed() {
    let first = test_state().await;
    let claim_path = first.idempotency_keys_path.clone();
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock_with_failures(1).await;
    configure_slack_events(&first, &api_base_url).await;
    seed_governed_slack_identity(&first).await;
    let first_provider = install_governed_slack_provider(&first, 0).await;
    let request_timestamp = chrono::Utc::now().timestamp();
    let response = app_router(first.clone())
        .oneshot(signed_slack_event_request(
            "Ev-durable-replay",
            SLACK_USER,
            "1800000030.000001",
            None,
            request_timestamp,
        ))
        .await
        .expect("initial durable event response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_counter(&slack_mock.attempts, 1, "initial Slack delivery").await;
    wait_for_slack_tasks(&first).await;
    assert_eq!(first_provider.calls.load(Ordering::SeqCst), 1);
    assert!(slack_mock.posts.lock().await.is_empty());

    let mut restarted = test_state().await;
    restarted.idempotency_keys_path = claim_path.clone();
    configure_slack_events(&restarted, &api_base_url).await;
    seed_governed_slack_identity(&restarted).await;
    let replay_provider = install_governed_slack_provider(&restarted, 0).await;
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    assert!(
        super::super::slack_interactions::start_slack_event_recovery_worker(&restarted)
            .await
            .expect("start Slack recovery worker")
    );
    wait_for_posts(&slack_mock, 1).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let replay_audit = crate::audit::load_protected_audit_events_for_tenant(
                &restarted,
                &TenantContext::explicit(ORG_ID, WORKSPACE_ID, None),
            )
            .await;
            if replay_audit.iter().any(|event| {
                event.event_type == "channel.slack.response.delivered"
                    && event.payload.get("replayed_staged_response") == Some(&Value::Bool(true))
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("autonomous Slack replay audit timeout");
    restarted
        .slack_event_tasks
        .shutdown(Duration::from_secs(1))
        .await;
    assert_eq!(slack_mock.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(replay_provider.calls.load(Ordering::SeqCst), 0);
    let replay_audit = crate::audit::load_protected_audit_events_for_tenant(
        &restarted,
        &TenantContext::explicit(ORG_ID, WORKSPACE_ID, None),
    )
    .await;
    assert!(replay_audit.iter().any(|event| {
        event.event_type == "channel.slack.response.delivered"
            && event.payload.get("replayed_staged_response") == Some(&Value::Bool(true))
    }));

    let mut completed_restart = test_state().await;
    completed_restart.idempotency_keys_path = claim_path;
    configure_slack_events(&completed_restart, &api_base_url).await;
    seed_governed_slack_identity(&completed_restart).await;
    let completed_provider = install_governed_slack_provider(&completed_restart, 0).await;
    let response = app_router(completed_restart.clone())
        .oneshot(signed_slack_event_request(
            "Ev-durable-replay",
            SLACK_USER,
            "1800000030.000001",
            None,
            request_timestamp,
        ))
        .await
        .expect("completed duplicate response");
    assert_eq!(response.status(), StatusCode::OK);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(slack_mock.attempts.load(Ordering::SeqCst), 2);
    assert_eq!(completed_provider.calls.load(Ordering::SeqCst), 0);
    assert!(completed_restart.storage.list_sessions().await.is_empty());
    mock_task.abort();
}

#[tokio::test]
async fn slack_outbound_rejects_bot_token_bound_to_another_team() {
    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    *slack_mock.auth_team_id.lock().await = "T_OTHER".to_string();
    configure_slack_events(&state, &api_base_url).await;
    seed_governed_slack_identity(&state).await;
    let provider = install_governed_slack_provider(&state, 0).await;

    let response = app_router(state.clone())
        .oneshot(signed_slack_event_request(
            "Ev-wrong-bot-team",
            SLACK_USER,
            "1800000035.000001",
            None,
            chrono::Utc::now().timestamp(),
        ))
        .await
        .expect("wrong bot team response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_counter(&slack_mock.auth_attempts, 1, "Slack auth.test").await;
    wait_for_slack_tasks(&state).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(slack_mock.attempts.load(Ordering::SeqCst), 0);
    assert!(slack_mock.posts.lock().await.is_empty());
    mock_task.abort();
}

#[tokio::test]
async fn slack_outbound_rejects_bot_token_bound_to_another_app() {
    let state = test_state().await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    *slack_mock.auth_app_id.lock().await = "A_OTHER".to_string();
    configure_slack_events(&state, &api_base_url).await;
    seed_governed_slack_identity(&state).await;
    let provider = install_governed_slack_provider(&state, 0).await;

    let response = app_router(state.clone())
        .oneshot(signed_slack_event_request(
            "Ev-wrong-bot-app",
            SLACK_USER,
            "1800000036.000001",
            None,
            chrono::Utc::now().timestamp(),
        ))
        .await
        .expect("wrong bot app response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_counter(
        &slack_mock.bots_info_attempts,
        1,
        "Slack bots.info app binding",
    )
    .await;
    wait_for_slack_tasks(&state).await;
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(slack_mock.attempts.load(Ordering::SeqCst), 0);
    assert!(slack_mock.posts.lock().await.is_empty());
    mock_task.abort();
}

#[tokio::test]
async fn crash_window_checkpoint_never_reruns_model_after_shutdown() {
    let first = test_state().await;
    let claim_path = first.idempotency_keys_path.clone();
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    configure_slack_events(&first, &api_base_url).await;
    seed_governed_slack_identity(&first).await;
    let blocked_provider = install_governed_slack_provider(&first, 0).await;
    blocked_provider
        .block_until_cancel
        .store(true, Ordering::SeqCst);
    let request_timestamp = chrono::Utc::now().timestamp();
    let response = app_router(first.clone())
        .oneshot(signed_slack_event_request(
            "Ev-shutdown-retry",
            SLACK_USER,
            "1800000040.000001",
            None,
            request_timestamp,
        ))
        .await
        .expect("tracked Slack response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_counter(&blocked_provider.calls, 1, "blocked provider start").await;
    assert_eq!(first.slack_event_tasks.active_count().await, 1);
    first
        .slack_event_tasks
        .shutdown(Duration::from_secs(1))
        .await;
    assert_eq!(first.slack_event_tasks.active_count().await, 0);
    assert_eq!(slack_mock.attempts.load(Ordering::SeqCst), 0);

    let mut restarted = test_state().await;
    restarted.idempotency_keys_path = claim_path;
    configure_slack_events(&restarted, &api_base_url).await;
    seed_governed_slack_identity(&restarted).await;
    let resumed_provider = install_governed_slack_provider(&restarted, 0).await;
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    let response = app_router(restarted.clone())
        .oneshot(signed_slack_event_request(
            "Ev-shutdown-retry",
            SLACK_USER,
            "1800000040.000001",
            None,
            request_timestamp,
        ))
        .await
        .expect("checkpointed Slack response");
    assert_eq!(response.status(), StatusCode::OK);
    wait_for_posts(&slack_mock, 1).await;
    wait_for_slack_tasks(&restarted).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(resumed_provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(slack_mock.attempts.load(Ordering::SeqCst), 1);
    mock_task.abort();
}
