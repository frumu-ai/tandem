// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Feature-gated persistent runner for the five-profile ACME Slack demo.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use async_trait::async_trait;
use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::{stream, Stream};
use hmac::{Hmac, Mac};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::Sha256;
use tandem_memory::db::MemoryDatabase;
use tandem_memory::types::GlobalMemoryRecord;
use tandem_providers::{ChatMessage, Provider, StreamChunk};
use tandem_types::{
    ModelInfo, PolicyDecisionEffect, ProviderInfo, TenantContext, ToolMode, ToolSchema,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::harness::DEMO_SLACK_CHANNEL_ID;
use super::{
    acme_demo_dataset, AcmeDemoDataset, DemoMemoryRow, DEMO_ORG_ID, DEMO_PROMPT, DEMO_SLACK_APP_ID,
    DEMO_SLACK_TEAM_ID, DEMO_WORKSPACE_ID,
};
use crate::governance_store::{for_state, GovernanceStoreFile};
use crate::{build_router_with_extensions, AppState};

const SIGNING_SECRET: &str = "acme-demo-signing-secret";
const PROVIDER_ID: &str = "acme-demo-deterministic";
const MODEL_ID: &str = "acme-demo-deterministic-1";
const MEMORY_ID_PREFIX: &str = "acme-live-";

#[derive(Debug, Serialize)]
pub struct AcmeLiveDemoReport {
    pub ok: bool,
    pub reset: AcmeResetReport,
    pub receipt_count: usize,
    pub receipt_run_ids: Vec<String>,
    pub slack_post_count: usize,
    pub approval_decision_ids: Vec<String>,
    pub approval_evidence_receipt_run_ids: Vec<String>,
    pub state_root: String,
}

#[derive(Debug, Default, Serialize)]
pub struct AcmeResetReport {
    pub sessions: usize,
    pub context_runs: usize,
    pub memory_rows: usize,
    pub policy_decisions: usize,
}

#[derive(Clone, Default)]
struct SlackMock {
    posts: Arc<Mutex<Vec<Value>>>,
}

async fn slack_auth() -> Json<Value> {
    Json(json!({
        "ok": true,
        "team_id": DEMO_SLACK_TEAM_ID,
        "app_id": DEMO_SLACK_APP_ID,
        "bot_id": "B_ACME_TANDEM",
        "user_id": "U_ACME_TANDEM",
        "is_bot": true
    }))
}

async fn slack_bot() -> Json<Value> {
    Json(json!({
        "ok": true,
        "bot": { "id": "B_ACME_TANDEM", "app_id": DEMO_SLACK_APP_ID }
    }))
}

async fn slack_post(State(state): State<SlackMock>, Json(payload): Json<Value>) -> Json<Value> {
    state.posts.lock().await.push(payload.clone());
    Json(json!({
        "ok": true,
        "channel": payload.get("channel").and_then(Value::as_str).unwrap_or_default(),
        "ts": "1800000000.000001"
    }))
}

async fn start_slack_mock() -> anyhow::Result<(String, SlackMock, tokio::task::JoinHandle<()>)> {
    let state = SlackMock::default();
    let app = Router::new()
        .route("/auth.test", get(slack_auth))
        .route("/bots.info", get(slack_bot))
        .route("/chat.postMessage", post(slack_post))
        .with_state(state.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((format!("http://{address}"), state, task))
}

struct DemoProvider {
    finance_calls: Arc<AtomicUsize>,
}

fn row_marker(row_id: &str) -> String {
    format!(
        "ACME-MARKER-{}",
        row_id.to_ascii_uppercase().replace('_', "-")
    )
}

#[async_trait]
impl Provider for DemoProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: PROVIDER_ID.to_string(),
            name: "ACME deterministic live demo".to_string(),
            models: vec![ModelInfo {
                id: MODEL_ID.to_string(),
                provider_id: PROVIDER_ID.to_string(),
                display_name: "ACME deterministic live demo".to_string(),
                context_window: 32_768,
            }],
        }
    }

    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        Ok("ACME governed answer".to_string())
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        _sampling: tandem_types::SamplingParams,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let prompt = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let offered_tools = _tools
            .unwrap_or_default()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        if offered_tools
            .iter()
            .any(|tool| tool == "mcp.invoices.read_invoices")
            && prompt.contains(&row_marker("finance_invoice_acme"))
            && !prompt.contains("deterministic ACME output")
            && !prompt.contains("paused by runtime approval gate")
            && self.finance_calls.fetch_add(1, Ordering::SeqCst) == 0
        {
            let call_id = "call_acme_live_finance".to_string();
            return Ok(Box::pin(stream::iter(vec![
                Ok(StreamChunk::ToolCallStart {
                    id: call_id.clone(),
                    name: "mcp.invoices.read_invoices".to_string(),
                }),
                Ok(StreamChunk::ToolCallDelta {
                    id: call_id.clone(),
                    args_delta: "{}".to_string(),
                }),
                Ok(StreamChunk::ToolCallEnd { id: call_id }),
                Ok(StreamChunk::Done {
                    finish_reason: "tool_calls".to_string(),
                    usage: None,
                }),
            ])));
        }
        let mut markers = acme_demo_dataset()
            .memory_rows
            .iter()
            .map(|row| row_marker(row.id))
            .filter(|marker| prompt.contains(marker))
            .collect::<Vec<_>>();
        markers.sort();
        let answer = format!("Governed ACME update: {}", markers.join(" "));
        Ok(Box::pin(stream::iter(vec![
            Ok(StreamChunk::TextDelta(answer)),
            Ok(StreamChunk::Done {
                finish_reason: "stop".to_string(),
                usage: None,
            }),
        ])))
    }
}

struct DemoTool {
    schema: ToolSchema,
    executions: Arc<AtomicUsize>,
}

#[async_trait]
impl tandem_tools::Tool for DemoTool {
    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<tandem_types::ToolResult> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        Ok(tandem_types::ToolResult {
            output: format!("deterministic ACME output for {}", self.schema.name),
            metadata: json!({ "demo": "acme" }),
        })
    }
}

pub async fn reset_and_run(state: &AppState) -> anyhow::Result<AcmeLiveDemoReport> {
    let _auth_mode = RuntimeAuthModeGuard::strict();
    let dataset = acme_demo_dataset();
    let reset = reset_seeded_state(state, &dataset).await?;
    seed_authority(state, &dataset).await?;
    seed_memory(state, &dataset).await?;
    let finance_executions = register_tools(state, &dataset).await;
    let finance_calls = Arc::new(AtomicUsize::new(0));
    state
        .providers
        .replace_for_test(
            vec![Arc::new(DemoProvider {
                finance_calls: finance_calls.clone(),
            })],
            Some(PROVIDER_ID.to_string()),
        )
        .await;
    // The runner's API listener is loopback-only and carries explicit ACME
    // tenant headers solely to verify the same list endpoint the UI consumes.
    state
        .trust_test_tenant_headers
        .store(true, Ordering::Relaxed);

    let (slack_url, slack, slack_task) = start_slack_mock().await?;
    configure_slack(state, &slack_url, &dataset).await?;
    let journaler = tokio::spawn(crate::run_session_context_run_journaler(state.clone()));
    let (server_url, server_task) = start_demo_server(state.clone()).await?;
    tokio::time::sleep(Duration::from_millis(75)).await;

    let result = run_profiles_and_read_receipts(state, &dataset, &server_url, &slack).await;
    state
        .slack_event_tasks
        .shutdown(Duration::from_secs(5))
        .await;
    server_task.abort();
    journaler.abort();
    slack_task.abort();
    let (
        receipt_run_ids,
        slack_post_count,
        approval_decision_ids,
        approval_evidence_receipt_run_ids,
    ) = result?;
    if finance_calls.load(Ordering::SeqCst) == 0 {
        bail!("Finance provider did not request the approval-gated tool");
    }
    if finance_executions.load(Ordering::SeqCst) != 0 {
        bail!("Finance approval-gated tool executed without approval");
    }
    Ok(AcmeLiveDemoReport {
        ok: true,
        reset,
        receipt_count: receipt_run_ids.len(),
        receipt_run_ids,
        slack_post_count,
        approval_decision_ids,
        approval_evidence_receipt_run_ids,
        state_root: context_runs_data_root(state).display().to_string(),
    })
}

async fn start_demo_server(
    state: AppState,
) -> anyhow::Result<(String, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = build_router_with_extensions(state, &[]);
    let task = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    Ok((format!("http://{address}"), task))
}

async fn configure_slack(
    state: &AppState,
    api_base_url: &str,
    dataset: &AcmeDemoDataset,
) -> anyhow::Result<()> {
    let allowed_users = dataset
        .profiles
        .iter()
        .map(|profile| profile.slack_user_id)
        .collect::<Vec<_>>();
    state
        .config
        .patch_project(json!({
            "channels": { "slack": {
                "signing_secret": SIGNING_SECRET,
                "events_enabled": true,
                "bot_token": "xoxb-acme-demo",
                "channel_id": DEMO_SLACK_CHANNEL_ID,
                "team_id": DEMO_SLACK_TEAM_ID,
                "app_id": DEMO_SLACK_APP_ID,
                "allowed_users": allowed_users,
                "api_base_url": api_base_url,
                "model_provider_id": PROVIDER_ID,
                "model_id": MODEL_ID,
                "security_profile": "trusted_team",
                "tenant": { "org_id": DEMO_ORG_ID, "workspace_id": DEMO_WORKSPACE_ID }
            }}
        }))
        .await?;
    Ok(())
}

async fn run_profiles_and_read_receipts(
    state: &AppState,
    dataset: &AcmeDemoDataset,
    server_url: &str,
    slack: &SlackMock,
) -> anyhow::Result<(Vec<String>, usize, Vec<String>, Vec<String>)> {
    let client = reqwest::Client::new();
    let nonce = uuid::Uuid::new_v4();
    let started_at_ms = crate::now_ms();
    for (index, profile) in dataset.profiles.iter().enumerate() {
        let body = json!({
            "type": "event_callback",
            "event_id": format!("Ev-acme-live-{nonce}-{index}"),
            "team_id": DEMO_SLACK_TEAM_ID,
            "api_app_id": DEMO_SLACK_APP_ID,
            "event": {
                "type": "message",
                "user": profile.slack_user_id,
                "channel": DEMO_SLACK_CHANNEL_ID,
                "text": DEMO_PROMPT,
                "ts": format!("1800000400.{index:06}")
            }
        })
        .to_string();
        let timestamp = chrono::Utc::now().timestamp();
        let response = client
            .post(format!("{server_url}/channels/slack/events"))
            .header("content-type", "application/json")
            .header("x-slack-request-timestamp", timestamp.to_string())
            .header("x-slack-signature", sign_slack(timestamp, body.as_bytes()))
            .body(body)
            .send()
            .await?;
        if !response.status().is_success() {
            bail!(
                "Slack ingress for {} returned {}",
                profile.slack_user_id,
                response.status()
            );
        }
    }
    wait_for_posts(slack, 5).await?;
    let approval_decision_ids = wait_for_approval_evidence(state, started_at_ms).await?;
    std::env::remove_var("TANDEM_RUNTIME_AUTH_MODE");
    let runs = wait_for_receipts(&client, server_url).await?;
    let mut ids = runs
        .iter()
        .filter_map(|run| {
            run.get("run_id")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    ids.sort();
    if ids.len() != 5 {
        bail!(
            "expected exactly five persisted ACME receipts, found {}",
            ids.len()
        );
    }
    let evidence_run_ids =
        verify_receipt_approval_evidence(&client, server_url, &ids, &approval_decision_ids).await?;
    Ok((
        ids,
        slack.posts.lock().await.len(),
        approval_decision_ids,
        evidence_run_ids,
    ))
}

async fn verify_receipt_approval_evidence(
    client: &reqwest::Client,
    server_url: &str,
    receipt_ids: &[String],
    decision_ids: &[String],
) -> anyhow::Result<Vec<String>> {
    let mut matching_runs = Vec::new();
    for run_id in receipt_ids {
        let response = client
            .get(format!(
                "{server_url}/context/runs/{run_id}/governance-evidence"
            ))
            .header("x-tandem-org-id", DEMO_ORG_ID)
            .header("x-tandem-workspace-id", DEMO_WORKSPACE_ID)
            .header("x-tandem-actor-id", "acme-demo-receipt-reader")
            .send()
            .await?;
        if !response.status().is_success() {
            bail!(
                "governance evidence for {run_id} returned {}",
                response.status()
            );
        }
        let payload: Value = response.json().await?;
        let package = payload.get("evidence_package").unwrap_or(&Value::Null);
        let has_decision = package
            .get("policy_decisions")
            .and_then(Value::as_array)
            .is_some_and(|decisions| {
                decisions.iter().any(|decision| {
                    decision
                        .get("decision_id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| decision_ids.iter().any(|expected| expected == id))
                })
            });
        let has_audit = package
            .pointer("/audit/protected_events")
            .and_then(Value::as_array)
            .is_some_and(|events| {
                events.iter().any(|event| {
                    event.get("event_type").and_then(Value::as_str)
                        == Some("approval.gate.approval_required")
                })
            });
        if has_decision && has_audit {
            matching_runs.push(run_id.clone());
        }
    }
    if matching_runs.is_empty() {
        bail!("no persisted receipt correlated the Finance approval decision and protected audit");
    }
    Ok(matching_runs)
}

async fn wait_for_approval_evidence(
    state: &AppState,
    started_at_ms: u64,
) -> anyhow::Result<Vec<String>> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let decisions = state.list_policy_decisions(&demo_tenant(), 100).await;
            let ids = decisions
                .iter()
                .filter(|decision| {
                    decision.tool.as_deref() == Some("mcp.invoices.read_invoices")
                        && decision.policy_id.as_deref() == Some("approval_gate_matrix")
                        && decision.decision == PolicyDecisionEffect::ApprovalRequired
                        && decision.created_at_ms >= started_at_ms
                })
                .map(|decision| decision.decision_id.clone())
                .collect::<Vec<_>>();
            if !ids.is_empty() {
                let audit =
                    crate::audit::load_protected_audit_events_for_tenant(state, &demo_tenant())
                        .await;
                if ids.iter().all(|decision_id| {
                    audit.iter().any(|event| {
                        event.event_type == "approval.gate.approval_required"
                            && event.payload.get("decision_id").and_then(Value::as_str)
                                == Some(decision_id.as_str())
                    })
                }) {
                    return ids;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("approval decision and protected audit evidence did not persist")
}

async fn wait_for_posts(slack: &SlackMock, expected: usize) -> anyhow::Result<()> {
    tokio::time::timeout(Duration::from_secs(15), async {
        while slack.posts.lock().await.len() < expected {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("mock Slack delivery timed out")
}

async fn wait_for_receipts(
    client: &reqwest::Client,
    server_url: &str,
) -> anyhow::Result<Vec<Value>> {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let response = client
                .get(format!(
                    "{server_url}/context/runs?run_type=session&source=channel:slack&limit=50"
                ))
                .header("x-tandem-org-id", DEMO_ORG_ID)
                .header("x-tandem-workspace-id", DEMO_WORKSPACE_ID)
                .header("x-tandem-actor-id", "acme-demo-receipt-reader")
                .send()
                .await?;
            let payload: Value = response.json().await?;
            let runs = payload
                .get("runs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(is_acme_context_run)
                .collect::<Vec<_>>();
            if runs.len() >= 5 {
                return Ok(runs);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .context("context-run receipts did not persist")?
}

fn sign_slack(timestamp: i64, body: &[u8]) -> String {
    let mut mac =
        <Hmac<Sha256> as Mac>::new_from_slice(SIGNING_SECRET.as_bytes()).expect("static HMAC key");
    mac.update(b"v0:");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b":");
    mac.update(body);
    let bytes = mac.finalize().into_bytes();
    let mut output = String::with_capacity(3 + bytes.len() * 2);
    output.push_str("v0=");
    for byte in bytes {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

async fn seed_memory(state: &AppState, dataset: &AcmeDemoDataset) -> anyhow::Result<()> {
    if let Some(parent) = state.memory_db_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let db = MemoryDatabase::new(&state.memory_db_path).await?;
    for row in &dataset.memory_rows {
        db.put_global_memory_record(&demo_memory_record(row, crate::now_ms()))
            .await?;
    }
    Ok(())
}

fn demo_memory_record(row: &DemoMemoryRow, now_ms: u64) -> GlobalMemoryRecord {
    let mut metadata = row.put_metadata();
    metadata["memory_trust"] = json!({ "label": "verified" });
    GlobalMemoryRecord {
        id: format!("{MEMORY_ID_PREFIX}{}", row.id),
        user_id: row.subject.clone(),
        source_type: "channel_message".to_string(),
        content: format!("{} {}", row.summary, row_marker(row.id)),
        content_hash: format!("{MEMORY_ID_PREFIX}hash-{}", row.id),
        run_id: "acme-live-seed".to_string(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: None,
        channel_tag: Some(DEMO_SLACK_CHANNEL_ID.to_string()),
        host_tag: None,
        metadata: Some(metadata),
        provenance: Some(json!({ "tenant_context": demo_tenant() })),
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "private".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        expires_at_ms: None,
    }
}

async fn register_tools(state: &AppState, dataset: &AcmeDemoDataset) -> Arc<AtomicUsize> {
    let finance_executions = Arc::new(AtomicUsize::new(0));
    for tool in &dataset.tools {
        let executions = if tool.schema.name == "mcp.invoices.read_invoices" {
            finance_executions.clone()
        } else {
            Arc::new(AtomicUsize::new(0))
        };
        state
            .tools
            .register_tool(
                tool.schema.name.clone(),
                Arc::new(DemoTool {
                    schema: tool.schema.clone(),
                    executions,
                }),
            )
            .await;
    }
    finance_executions
}

struct RuntimeAuthModeGuard(Option<String>);

impl RuntimeAuthModeGuard {
    fn strict() -> Self {
        let previous = std::env::var("TANDEM_RUNTIME_AUTH_MODE").ok();
        std::env::set_var("TANDEM_RUNTIME_AUTH_MODE", "hosted_single_tenant");
        Self(previous)
    }
}

impl Drop for RuntimeAuthModeGuard {
    fn drop(&mut self) {
        match self.0.take() {
            Some(previous) => std::env::set_var("TANDEM_RUNTIME_AUTH_MODE", previous),
            None => std::env::remove_var("TANDEM_RUNTIME_AUTH_MODE"),
        }
    }
}

async fn seed_authority(state: &AppState, dataset: &AcmeDemoDataset) -> anyhow::Result<()> {
    state.enterprise.org_units.write().await.extend(
        dataset
            .graph
            .units
            .iter()
            .cloned()
            .map(|row| (row.unit_id.clone(), row)),
    );
    state.enterprise.org_unit_memberships.write().await.extend(
        dataset
            .graph
            .memberships
            .iter()
            .cloned()
            .map(|row| (row.membership_id.clone(), row)),
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
                .map(|row| (row.grant_id.clone(), row)),
        );
    persist_authority(state).await
}

async fn persist_authority(state: &AppState) -> anyhow::Result<()> {
    let units = state.enterprise.org_units.read().await;
    let memberships = state.enterprise.org_unit_memberships.read().await;
    let grants = state.enterprise.org_unit_access_grants.read().await;
    let unit_records = units
        .iter()
        .map(|(key, row)| {
            GovernanceStoreFile::OrgUnits.json_record(key, row, &row.tenant_context, Some(key))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let membership_records = memberships
        .iter()
        .map(|(key, row)| {
            GovernanceStoreFile::OrgUnitMemberships.json_record(
                key,
                row,
                &row.tenant_context,
                Some(&row.unit.id),
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let grant_records = grants
        .iter()
        .map(|(key, row)| {
            GovernanceStoreFile::OrgUnitAccessGrants.json_record(
                key,
                row,
                &row.tenant_context,
                Some(&row.unit.id),
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let store = for_state(state);
    store
        .write_json_records(GovernanceStoreFile::OrgUnits, &unit_records)
        .await?;
    store
        .write_json_records(GovernanceStoreFile::OrgUnitMemberships, &membership_records)
        .await?;
    store
        .write_json_records(GovernanceStoreFile::OrgUnitAccessGrants, &grant_records)
        .await?;
    Ok(())
}

async fn reset_seeded_state(
    state: &AppState,
    dataset: &AcmeDemoDataset,
) -> anyhow::Result<AcmeResetReport> {
    let actor_ids = dataset
        .profiles
        .iter()
        .map(|profile| profile.actor_id.as_str())
        .collect::<Vec<_>>();
    let sessions = state.storage.list_sessions().await;
    let mut removed_sessions = 0;
    let mut removed_session_ids = Vec::new();
    for session in sessions {
        if session.tenant_context.org_id == DEMO_ORG_ID
            && actor_ids.contains(
                &session
                    .tenant_context
                    .actor_id
                    .as_deref()
                    .unwrap_or_default(),
            )
        {
            removed_session_ids.push(session.id.clone());
            removed_sessions += usize::from(state.storage.delete_session(&session.id).await?);
        }
    }

    let removed_policy_decisions = {
        let mut decisions = state.policy_decisions.write().await;
        let before = decisions.len();
        decisions.retain(|_, decision| {
            !decision
                .session_id
                .as_ref()
                .is_some_and(|session_id| removed_session_ids.contains(session_id))
        });
        before.saturating_sub(decisions.len())
    };
    if removed_policy_decisions > 0 {
        state.persist_policy_decisions().await?;
    }

    let db = MemoryDatabase::new(&state.memory_db_path).await?;
    let mut removed_memory = 0;
    for row in &dataset.memory_rows {
        removed_memory += usize::from(
            db.delete_global_memory(&format!("{MEMORY_ID_PREFIX}{}", row.id))
                .await?,
        );
    }
    let removed_runs = reset_context_runs(state).await?;
    Ok(AcmeResetReport {
        sessions: removed_sessions,
        context_runs: removed_runs,
        memory_rows: removed_memory,
        policy_decisions: removed_policy_decisions,
    })
}

async fn reset_context_runs(state: &AppState) -> anyhow::Result<usize> {
    let root = context_runs_data_root(state).join("hot");
    let mut entries = match tokio::fs::read_dir(&root).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    let mut removed = 0;
    while let Some(entry) = entries.next_entry().await? {
        let state_path = entry.path().join("run_state.json");
        let value = match tokio::fs::read_to_string(state_path).await {
            Ok(raw) => serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null),
            Err(_) => continue,
        };
        if is_acme_context_run(&value) {
            tokio::fs::remove_dir_all(entry.path()).await?;
            removed += 1;
        }
    }
    Ok(removed)
}

fn is_acme_context_run(value: &Value) -> bool {
    value
        .pointer("/tenant_context/org_id")
        .and_then(Value::as_str)
        == Some(DEMO_ORG_ID)
        && value
            .pointer("/tenant_context/workspace_id")
            .and_then(Value::as_str)
            == Some(DEMO_WORKSPACE_ID)
        && value.get("source_client").and_then(Value::as_str) == Some("channel:slack")
        && value
            .pointer("/source_metadata/slack_team_id")
            .and_then(Value::as_str)
            == Some(DEMO_SLACK_TEAM_ID)
        && value
            .pointer("/source_metadata/slack_channel_id")
            .and_then(Value::as_str)
            == Some(DEMO_SLACK_CHANNEL_ID)
}

fn context_runs_data_root(state: &AppState) -> PathBuf {
    if let Some(parent) = state.shared_resources_path.parent() {
        if parent.file_name().and_then(|value| value.to_str()) == Some("system") {
            if let Some(data_dir) = parent.parent() {
                return data_dir.join("context-runs");
            }
        }
        return parent.join("context-runs");
    }
    Path::new(".tandem").join("data").join("context-runs")
}

fn demo_tenant() -> TenantContext {
    TenantContext::explicit(DEMO_ORG_ID, DEMO_WORKSPACE_ID, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_matcher_requires_all_acme_dimensions() {
        let receipt = json!({
            "tenant_context": { "org_id": DEMO_ORG_ID, "workspace_id": DEMO_WORKSPACE_ID },
            "source_client": "channel:slack",
            "source_metadata": {
                "slack_team_id": DEMO_SLACK_TEAM_ID,
                "slack_channel_id": DEMO_SLACK_CHANNEL_ID
            }
        });
        assert!(is_acme_context_run(&receipt));
        let mut unrelated = receipt;
        unrelated["source_metadata"]["slack_channel_id"] = json!("C_OTHER");
        assert!(!is_acme_context_run(&unrelated));
    }
}
