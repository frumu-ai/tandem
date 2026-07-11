//! TAN-682: production-path five-profile ACME Slack governance E2E.
//!
//! Unlike the receipt-shape fixture in `crate::acme_demo::harness` (which
//! synthesizes JSON from the seeded dataset), these tests execute the real
//! production path for every profile: the seeded authority graph and governed
//! memory records live in the actual stores, the same signed Slack prompt
//! enters through `POST /channels/slack/events`, identity resolution builds a
//! real `VerifiedTenantContext`, the engine loop runs against a deterministic
//! provider whose answer is derived from the memory that was actually injected
//! into its prompt, the response is delivered to a mock Slack API, and the
//! assertions read back persisted evidence (protected audit, policy decisions,
//! session state) through production paths.
//!
//! Single command (also part of required CI via the workspace nextest run):
//!
//! `cargo test -p tandem-server acme_slack_demo --lib`

use super::slack_events::{
    configure_slack_events_for_installation, seed_acme_demo_authority,
    signed_slack_event_request_with_text, start_slack_api_mock, wait_for_posts,
    wait_for_slack_tasks,
};
use super::*;

use async_trait::async_trait;
use futures::{stream, Stream};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tandem_memory::db::MemoryDatabase;
use tandem_memory::types::GlobalMemoryRecord;
use tandem_providers::{ChatMessage, Provider, StreamChunk};
use tandem_types::{
    ModelInfo, PolicyDecisionEffect, ProviderInfo, TenantContext, ToolMode, ToolSchema,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::acme_demo::harness::DEMO_SLACK_CHANNEL_ID;
use crate::acme_demo::{
    acme_demo_dataset, AcmeDemoDataset, DemoMemoryRow, DemoProfile, DEMO_ORG_ID, DEMO_PROMPT,
    DEMO_SLACK_APP_ID, DEMO_SLACK_TEAM_ID, DEMO_WORKSPACE_ID,
};

/// The searchable marker token seeded into one memory row's content. Marker
/// presence in provider prompt context / Slack responses is what turns "the
/// engine retrieved real memory" into an assertable fact per department.
fn row_marker(row_id: &str) -> String {
    format!(
        "ACME-MARKER-{}",
        row_id.to_ascii_uppercase().replace('_', "-")
    )
}

fn demo_tenant() -> TenantContext {
    TenantContext::explicit(DEMO_ORG_ID, DEMO_WORKSPACE_ID, None)
}

/// Seed every dataset memory row into the real tenant memory store with the
/// exact metadata shape a governed `memory_put` stamps (`owner_org_unit_id` +
/// `classification` from `DemoMemoryRow::put_metadata`), collected by the
/// department member's channel actor.
async fn seed_acme_demo_memory(state: &AppState, dataset: &AcmeDemoDataset) {
    if let Some(parent) = state.memory_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let db = MemoryDatabase::new(&state.memory_db_path)
        .await
        .expect("open memory db for seeding");
    let now_ms = crate::now_ms();
    for row in &dataset.memory_rows {
        let record = demo_memory_record(row, now_ms);
        db.put_global_memory_record(&record)
            .await
            .unwrap_or_else(|err| panic!("seed memory row {}: {err:?}", row.id));
    }
}

fn demo_memory_record(row: &DemoMemoryRow, now_ms: u64) -> GlobalMemoryRecord {
    let mut metadata = row.put_metadata();
    metadata["memory_trust"] = json!({ "label": "verified" });
    GlobalMemoryRecord {
        id: format!("acme-e2e-{}", row.id),
        user_id: row.subject.clone(),
        source_type: "channel_message".to_string(),
        content: format!("{} {}", row.summary, row_marker(row.id)),
        content_hash: format!("acme-e2e-hash-{}", row.id),
        run_id: "acme-e2e-seed".to_string(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: None,
        channel_tag: None,
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

/// A registered demo tool: carries the dataset's exact `ToolSchema` (with its
/// security descriptor) so risk tiers and discovery behave like production,
/// and returns a deterministic output when executed.
struct AcmeDemoStubTool {
    schema: ToolSchema,
    executions: Arc<AtomicUsize>,
}

#[async_trait]
impl tandem_tools::Tool for AcmeDemoStubTool {
    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<tandem_types::ToolResult> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        Ok(tandem_types::ToolResult {
            output: format!("acme demo tool output for {}", self.schema.name),
            metadata: json!({}),
        })
    }
}

/// Register every dataset tool in the real tool registry; returns the
/// per-tool execution counters so tests can prove blocked tools never ran.
async fn register_acme_demo_tools(
    state: &AppState,
    dataset: &AcmeDemoDataset,
) -> std::collections::HashMap<String, Arc<AtomicUsize>> {
    let mut executions = std::collections::HashMap::new();
    for tool in &dataset.tools {
        let counter = Arc::new(AtomicUsize::new(0));
        executions.insert(tool.schema.name.clone(), counter.clone());
        state
            .tools
            .register_tool(
                tool.schema.name.clone(),
                Arc::new(AcmeDemoStubTool {
                    schema: tool.schema.clone(),
                    executions: counter,
                }),
            )
            .await;
    }
    executions
}

/// One provider dispatch, as the model actually saw it.
#[derive(Debug, Clone)]
struct AcmeProviderCall {
    prompt_text: String,
    offered_tools: Vec<String>,
}

#[derive(Clone, Default)]
struct AcmeE2EProbe {
    calls: Arc<Mutex<Vec<AcmeProviderCall>>>,
    finance_tool_calls_emitted: Arc<AtomicUsize>,
}

/// Deterministic provider: answers with exactly the memory markers present in
/// its prompt context, so the Slack-visible response is a function of what
/// governed retrieval actually injected. When `request_finance_tool` is set
/// and the prompt carries Finance memory, the first such dispatch emits a real
/// `mcp.invoices.read_invoices` tool call to drive the approval gate.
struct AcmeE2EProvider {
    probe: AcmeE2EProbe,
    request_finance_tool: bool,
}

fn markers_in(text: &str) -> Vec<String> {
    let mut found: Vec<String> = acme_demo_dataset()
        .memory_rows
        .iter()
        .map(|row| row_marker(row.id))
        .filter(|marker| text.contains(marker.as_str()))
        .collect();
    found.sort();
    found
}

#[async_trait]
impl Provider for AcmeE2EProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "governed-slack-test".to_string(),
            name: "ACME E2E Deterministic".to_string(),
            models: vec![ModelInfo {
                id: "governed-slack-test-1".to_string(),
                provider_id: "governed-slack-test".to_string(),
                display_name: "ACME E2E Deterministic 1".to_string(),
                context_window: 32_768,
            }],
        }
    }

    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        Ok("acme governed answer".to_string())
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        _sampling: tandem_types::SamplingParams,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let prompt_text = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let mut offered_tools = tools
            .unwrap_or_default()
            .into_iter()
            .map(|tool| tool.name)
            .collect::<Vec<_>>();
        offered_tools.sort();
        let finance_offered = offered_tools
            .iter()
            .any(|tool| tool == "mcp.invoices.read_invoices");
        self.probe.calls.lock().await.push(AcmeProviderCall {
            prompt_text: prompt_text.clone(),
            offered_tools,
        });

        if self.request_finance_tool
            && finance_offered
            && prompt_text.contains(&row_marker("finance_invoice_acme"))
            && !prompt_text.contains("acme demo tool output")
            && !prompt_text.contains("paused by runtime approval gate")
        {
            let call_index = self
                .probe
                .finance_tool_calls_emitted
                .fetch_add(1, Ordering::SeqCst);
            let call_id = format!("call_acme_finance_{call_index}");
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

        let answer = format!(
            "Governed ACME update: {}",
            markers_in(&prompt_text).join(" ")
        );
        Ok(Box::pin(stream::iter(vec![
            Ok(StreamChunk::TextDelta(answer)),
            Ok(StreamChunk::Done {
                finish_reason: "stop".to_string(),
                usage: None,
            }),
        ])))
    }
}

async fn install_acme_e2e_provider(state: &AppState, request_finance_tool: bool) -> AcmeE2EProbe {
    let probe = AcmeE2EProbe::default();
    state
        .providers
        .replace_for_test(
            vec![Arc::new(AcmeE2EProvider {
                probe: probe.clone(),
                request_finance_tool,
            })],
            Some("governed-slack-test".to_string()),
        )
        .await;
    probe
}

/// Seeds authority + memory + tools + Slack installation into one AppState and
/// returns everything a test needs to drive and interrogate the flow. This is
/// also the reset story: every invocation builds the seeded world from scratch
/// out of `acme_demo_dataset()`, so two runs of the harness are identical.
struct AcmeE2EWorld {
    state: AppState,
    dataset: AcmeDemoDataset,
    probe: AcmeE2EProbe,
    slack_mock: super::slack_events::SlackApiMock,
    mock_task: tokio::task::JoinHandle<()>,
    tool_executions: std::collections::HashMap<String, Arc<AtomicUsize>>,
}

async fn seed_acme_e2e_world(request_finance_tool: bool) -> AcmeE2EWorld {
    let state = test_state().await;
    let dataset = seed_acme_demo_authority(&state).await;
    seed_acme_demo_memory(&state, &dataset).await;
    let tool_executions = register_acme_demo_tools(&state, &dataset).await;
    let (api_base_url, slack_mock, mock_task) = start_slack_api_mock().await;
    *slack_mock.auth_team_id.lock().await = DEMO_SLACK_TEAM_ID.to_string();
    *slack_mock.auth_app_id.lock().await = DEMO_SLACK_APP_ID.to_string();
    let allowed_users = dataset
        .profiles
        .iter()
        .map(|profile| profile.slack_user_id)
        .collect::<Vec<_>>();
    configure_slack_events_for_installation(
        &state,
        &api_base_url,
        DEMO_SLACK_TEAM_ID,
        DEMO_SLACK_APP_ID,
        DEMO_SLACK_CHANNEL_ID,
        &allowed_users,
    )
    .await;
    let probe = install_acme_e2e_provider(&state, request_finance_tool).await;
    AcmeE2EWorld {
        state,
        dataset,
        probe,
        slack_mock,
        mock_task,
        tool_executions,
    }
}

/// Sign and submit the demo prompt for one profile through the production
/// Slack Events route.
async fn submit_demo_prompt(
    app: &axum::Router,
    profile: &DemoProfile,
    event_id: &str,
    message_ts: &str,
) {
    let response = app
        .clone()
        .oneshot(signed_slack_event_request_with_text(
            event_id,
            profile.slack_user_id,
            DEMO_SLACK_CHANNEL_ID,
            Some(DEMO_SLACK_TEAM_ID),
            Some(DEMO_SLACK_APP_ID),
            message_ts,
            None,
            chrono::Utc::now().timestamp(),
            DEMO_PROMPT,
        ))
        .await
        .expect("Slack event response");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "signed ingress must accept {}",
        profile.slack_user_id
    );
}

fn expected_markers(dataset: &AcmeDemoDataset, profile: &DemoProfile) -> Vec<String> {
    let now_ms = crate::now_ms();
    let mut markers: Vec<String> = dataset
        .memory_rows
        .iter()
        .filter(|row| crate::acme_demo::profile_can_read_memory(profile, row, now_ms))
        .map(|row| row_marker(row.id))
        .collect();
    markers.sort();
    markers
}

fn forbidden_markers(dataset: &AcmeDemoDataset, profile: &DemoProfile) -> Vec<String> {
    let expected = expected_markers(dataset, profile);
    dataset
        .memory_rows
        .iter()
        .map(|row| row_marker(row.id))
        .filter(|marker| !expected.contains(marker))
        .collect()
}

fn expected_offered_tools(dataset: &AcmeDemoDataset, profile: &DemoProfile) -> Vec<String> {
    let now_ms = crate::now_ms();
    let mut tools: Vec<String> = dataset
        .tools
        .iter()
        .filter(|tool| crate::acme_demo::profile_can_use_tool(dataset, profile, tool, now_ms))
        .map(|tool| tool.schema.name.clone())
        .collect();
    tools.sort();
    tools
}

/// Find the provider dispatch belonging to `profile` by its own memory marker.
/// Every profile has at least one reachable row and markers are department-
/// unique, so this classification is unambiguous once cross-department leakage
/// is ruled out (asserted by the caller).
fn call_for_profile<'a>(
    calls: &'a [AcmeProviderCall],
    dataset: &AcmeDemoDataset,
    profile: &DemoProfile,
) -> &'a AcmeProviderCall {
    let own_marker = expected_markers(dataset, profile)
        .first()
        .cloned()
        .expect("every profile reaches at least one memory row");
    calls
        .iter()
        .find(|call| call.prompt_text.contains(&own_marker))
        .unwrap_or_else(|| panic!("no provider dispatch carried {own_marker}"))
}

/// The full production-path five-profile run: signed ingress → verified
/// governed session → engine + deterministic provider over actually retrieved
/// memory → mock Slack delivery → persisted evidence.
#[tokio::test]
async fn acme_slack_demo_e2e_five_profiles_run_the_production_path() {
    let world = seed_acme_e2e_world(false).await;
    let app = app_router(world.state.clone());

    for (index, profile) in world.dataset.profiles.iter().enumerate() {
        submit_demo_prompt(
            &app,
            profile,
            &format!("Ev-acme-e2e-{index}"),
            &format!("1800000042.{index:06}"),
        )
        .await;
    }
    wait_for_posts(&world.slack_mock, world.dataset.profiles.len()).await;
    wait_for_slack_tasks(&world.state).await;

    let calls = world.probe.calls.lock().await.clone();
    assert_eq!(calls.len(), 5, "one provider dispatch per profile");
    let posts = world.slack_mock.posts.lock().await.clone();
    assert_eq!(posts.len(), 5, "one Slack delivery per profile");

    let sessions = world.state.storage.list_sessions().await;
    assert_eq!(sessions.len(), 5);

    for profile in &world.dataset.profiles {
        let expected = expected_markers(&world.dataset, profile);
        let forbidden = forbidden_markers(&world.dataset, profile);

        // (1) Each profile resolves a distinct real VerifiedTenantContext.
        let session = sessions
            .iter()
            .find(|session| session.tenant_context.actor_id.as_deref() == Some(&profile.actor_id))
            .unwrap_or_else(|| panic!("missing session for {}", profile.slack_user_id));
        let verified = session
            .verified_tenant_context
            .as_ref()
            .expect("verified context");
        assert_eq!(verified.org_units, profile.org_units());
        assert_eq!(
            verified.tenant_context.org_id, DEMO_ORG_ID,
            "verified tenant org"
        );

        // (2) The provider prompt carried the profile's in-scope memory and
        // no other department's marker data.
        let call = call_for_profile(&calls, &world.dataset, profile);
        for marker in &expected {
            assert!(
                call.prompt_text.contains(marker),
                "{}: in-scope memory {marker} missing from prompt context",
                profile.slack_user_id
            );
        }
        for marker in &forbidden {
            assert!(
                !call.prompt_text.contains(marker),
                "{}: forbidden marker {marker} leaked into prompt context",
                profile.slack_user_id
            );
        }

        // (3) Hidden tools are absent from the model tool schema itself.
        assert_eq!(
            call.offered_tools,
            expected_offered_tools(&world.dataset, profile),
            "{}: offered tool schemas must equal the profile's reachable tool set",
            profile.slack_user_id
        );

        // (4) The Slack-visible answer is derived from retrieved in-scope
        // memory and carries no other department's data, addressed to the
        // demo channel.
        let post = posts
            .iter()
            .find(|post| {
                post.get("text")
                    .and_then(Value::as_str)
                    .is_some_and(|text| expected.iter().all(|marker| text.contains(marker)))
            })
            .unwrap_or_else(|| panic!("no Slack post for {}", profile.slack_user_id));
        assert_eq!(
            post.get("channel").and_then(Value::as_str),
            Some(DEMO_SLACK_CHANNEL_ID)
        );
        let text = post.get("text").and_then(Value::as_str).unwrap_or_default();
        for marker in &forbidden {
            assert!(
                !text.contains(marker),
                "{}: forbidden marker {marker} leaked into the Slack response",
                profile.slack_user_id
            );
        }
        assert!(
            post.get("thread_ts").and_then(Value::as_str).is_some(),
            "Slack reply must be threaded"
        );
    }

    // (5) Persisted evidence is readable through the production audit path:
    // one attributed ingress-accept per profile, tenant-scoped.
    let audit =
        crate::audit::load_protected_audit_events_for_tenant(&world.state, &demo_tenant()).await;
    for profile in &world.dataset.profiles {
        assert!(
            audit.iter().any(|event| {
                event.event_type == "channel.slack.ingress.accepted"
                    && event.actor.as_deref() == Some(profile.actor_id.as_str())
            }),
            "missing persisted ingress-accepted audit for {}",
            profile.slack_user_id
        );
    }
    let unrelated = TenantContext::explicit("other-org", DEMO_WORKSPACE_ID, None);
    assert!(
        crate::audit::load_protected_audit_events_for_tenant(&world.state, &unrelated)
            .await
            .is_empty(),
        "demo audit evidence must stay tenant-scoped"
    );

    // (6) Duplicate Slack delivery of an already-processed event is absorbed
    // by the durable claim without a second run or Slack post.
    submit_demo_prompt(
        &app,
        &world.dataset.profiles[0],
        "Ev-acme-e2e-0",
        "1800000042.000000",
    )
    .await;
    wait_for_slack_tasks(&world.state).await;
    assert_eq!(
        world.probe.calls.lock().await.len(),
        5,
        "duplicate event must not re-run the engine"
    );
    assert_eq!(
        world.slack_mock.posts.lock().await.len(),
        5,
        "duplicate event must not re-deliver to Slack"
    );

    world.mock_task.abort();
}

fn demo_tenant_get(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("x-tandem-org-id", DEMO_ORG_ID)
        .header("x-tandem-workspace-id", DEMO_WORKSPACE_ID)
        .header("x-tandem-actor-id", "acme-receipt-reader")
        .body(Body::empty())
        .expect("tenant request")
}

async fn get_json(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(demo_tenant_get(uri))
        .await
        .expect("response");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, payload)
}

/// Poll `GET /context/runs` (production list API, tenant-scoped) until the
/// expected number of Slack-originated session receipts is visible.
async fn wait_for_slack_receipts(app: &axum::Router, expected: usize) -> Vec<Value> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let (status, payload) = get_json(
                app,
                "/context/runs?run_type=session&source=channel:slack&limit=50",
            )
            .await;
            if status == StatusCode::OK {
                let runs = payload
                    .get("runs")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if runs.len() >= expected {
                    return runs;
                }
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("Slack receipts did not persist in time")
}

/// TAN-686 substrate: running the production-path demo persists one
/// selectable, Slack-attributed context run per profile, readable through the
/// production `/context/runs` APIs with tenant isolation intact.
#[tokio::test]
async fn acme_slack_demo_e2e_persists_selectable_slack_receipts() {
    let world = seed_acme_e2e_world(false).await;
    let journaler = tokio::spawn(crate::run_session_context_run_journaler(
        world.state.clone(),
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let app = app_router(world.state.clone());

    for (index, profile) in world.dataset.profiles.iter().enumerate() {
        submit_demo_prompt(
            &app,
            profile,
            &format!("Ev-acme-receipt-{index}"),
            &format!("1800000300.{index:06}"),
        )
        .await;
    }
    wait_for_posts(&world.slack_mock, world.dataset.profiles.len()).await;
    wait_for_slack_tasks(&world.state).await;

    let runs = wait_for_slack_receipts(&app, world.dataset.profiles.len()).await;
    assert_eq!(runs.len(), 5, "five selectable Slack receipts");

    for profile in &world.dataset.profiles {
        let run = runs
            .iter()
            .find(|run| {
                run.pointer("/source_metadata/user_id")
                    .and_then(Value::as_str)
                    == Some(profile.slack_user_id)
            })
            .unwrap_or_else(|| panic!("no persisted receipt for {}", profile.slack_user_id));
        assert_eq!(
            run.get("source_client").and_then(Value::as_str),
            Some("channel:slack")
        );
        assert_eq!(
            run.pointer("/source_metadata/slack_channel_id")
                .and_then(Value::as_str),
            Some(DEMO_SLACK_CHANNEL_ID)
        );
        assert_eq!(
            run.pointer("/source_metadata/slack_team_id")
                .and_then(Value::as_str),
            Some(DEMO_SLACK_TEAM_ID)
        );
        assert_eq!(
            run.pointer("/tenant_context/org_id")
                .and_then(Value::as_str),
            Some(DEMO_ORG_ID)
        );
        let run_id = run
            .get("run_id")
            .and_then(Value::as_str)
            .expect("receipt run id");
        assert!(run_id.starts_with("session-"), "session-scoped receipt id");

        // The single-run and ledger production endpoints serve the receipt.
        let (status, _) = get_json(&app, &format!("/context/runs/{run_id}")).await;
        assert_eq!(status, StatusCode::OK, "context run readable");
        let (status, ledger) =
            get_json(&app, &format!("/context/runs/{run_id}/ledger?tail=200")).await;
        assert_eq!(status, StatusCode::OK, "ledger readable");
        assert!(ledger.get("tool_manifest").is_some());
    }

    // Tenant isolation: another tenant sees none of these receipts.
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/context/runs?run_type=session&source=channel:slack&limit=50")
                .header("x-tandem-org-id", "other-org")
                .header("x-tandem-workspace-id", DEMO_WORKSPACE_ID)
                .header("x-tandem-actor-id", "other-actor")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("runs")
            .and_then(Value::as_array)
            .map(Vec::len)
            .unwrap_or_default(),
        0,
        "Slack receipts must not leak across tenants"
    );

    journaler.abort();
    world.mock_task.abort();
}

/// Reset/replay: rebuilding the seeded world from the dataset and replaying
/// the same five prompts yields the same governed outcome — the harness is
/// deterministic end to end, not dependent on residue from a previous run.
#[tokio::test]
async fn acme_slack_demo_e2e_reset_and_replay_is_reproducible() {
    for round in 0..2 {
        let world = seed_acme_e2e_world(false).await;
        let app = app_router(world.state.clone());
        for (index, profile) in world.dataset.profiles.iter().enumerate() {
            submit_demo_prompt(
                &app,
                profile,
                &format!("Ev-acme-replay-{round}-{index}"),
                &format!("1800000100.{index:06}"),
            )
            .await;
        }
        wait_for_posts(&world.slack_mock, world.dataset.profiles.len()).await;
        wait_for_slack_tasks(&world.state).await;
        let calls = world.probe.calls.lock().await.clone();
        assert_eq!(calls.len(), 5, "round {round}: five dispatches");
        for profile in &world.dataset.profiles {
            let call = call_for_profile(&calls, &world.dataset, profile);
            assert_eq!(
                call.offered_tools,
                expected_offered_tools(&world.dataset, profile),
                "round {round}: tool scope must be reproducible for {}",
                profile.slack_user_id
            );
            for marker in forbidden_markers(&world.dataset, profile) {
                assert!(
                    !call.prompt_text.contains(&marker),
                    "round {round}: {} leaked {marker}",
                    profile.slack_user_id
                );
            }
        }
        world.mock_task.abort();
    }
}

/// Finance-sensitive actions enter the real approval gate: under a strict
/// runtime auth mode, the Finance profile's `mcp.invoices.read_invoices`
/// invocation is paused by the CT-20 action gate, which persists an
/// approval-required policy decision and hash-chained protected audit — and
/// the tool itself never executes.
#[tokio::test]
#[serial_test::serial]
async fn acme_slack_demo_e2e_finance_sensitive_tool_enters_the_real_approval_gate() {
    struct RuntimeAuthModeGuard(Option<String>);
    impl Drop for RuntimeAuthModeGuard {
        fn drop(&mut self) {
            match self.0.take() {
                Some(previous) => std::env::set_var("TANDEM_RUNTIME_AUTH_MODE", previous),
                None => std::env::remove_var("TANDEM_RUNTIME_AUTH_MODE"),
            }
        }
    }
    let _mode = RuntimeAuthModeGuard(std::env::var("TANDEM_RUNTIME_AUTH_MODE").ok());
    std::env::set_var("TANDEM_RUNTIME_AUTH_MODE", "hosted_single_tenant");

    let world = seed_acme_e2e_world(true).await;
    let journaler = tokio::spawn(crate::run_session_context_run_journaler(
        world.state.clone(),
    ));
    tokio::time::sleep(Duration::from_millis(50)).await;
    let app = app_router(world.state.clone());
    let finance = world
        .dataset
        .profiles
        .iter()
        .find(|profile| profile.unit_id == "finance")
        .expect("finance profile");

    submit_demo_prompt(&app, finance, "Ev-acme-finance-gate", "1800000200.000001").await;
    wait_for_posts(&world.slack_mock, 1).await;
    wait_for_slack_tasks(&world.state).await;

    assert!(
        world
            .probe
            .finance_tool_calls_emitted
            .load(Ordering::SeqCst)
            >= 1,
        "provider must have requested the finance tool"
    );
    assert_eq!(
        world.tool_executions["mcp.invoices.read_invoices"].load(Ordering::SeqCst),
        0,
        "the gated finance tool must never execute without approval"
    );

    // The gate decision is persisted through the production policy-decision
    // store, attributed to the demo tenant.
    let decisions = world.state.list_policy_decisions(&demo_tenant(), 100).await;
    let gate_decision = decisions
        .iter()
        .find(|decision| {
            decision.tool.as_deref() == Some("mcp.invoices.read_invoices")
                && decision.policy_id.as_deref() == Some("approval_gate_matrix")
        })
        .expect("approval-gate policy decision must be persisted");
    assert_eq!(
        gate_decision.decision,
        PolicyDecisionEffect::ApprovalRequired
    );
    assert_eq!(gate_decision.tenant_context.org_id, DEMO_ORG_ID);

    // ...and mirrored into the hash-chained protected audit ledger.
    let audit =
        crate::audit::load_protected_audit_events_for_tenant(&world.state, &demo_tenant()).await;
    assert!(
        audit.iter().any(|event| {
            event.event_type == "approval.gate.approval_required"
                && event.payload.get("decision_id").and_then(Value::as_str)
                    == Some(gate_decision.decision_id.as_str())
        }),
        "approval-required gate evidence must be in the protected audit ledger"
    );

    // The governed run is complete; drop back to local auth mode so the
    // header-tenant receipt reads work (hosted mode requires transport
    // tokens the test reader does not carry). The RAII guard still restores
    // the original value on exit.
    std::env::remove_var("TANDEM_RUNTIME_AUTH_MODE");

    // The receipt built from persisted production evidence correlates the
    // gate decision: the journaled Blocked ledger record carries the policy
    // decision id, so the governance-evidence export links it end to end.
    #[cfg(feature = "premium-governance")]
    {
        let receipts = wait_for_slack_receipts(&app, 1).await;
        let run_id = receipts[0]
            .get("run_id")
            .and_then(Value::as_str)
            .expect("finance receipt run id")
            .to_string();
        let (status, evidence) =
            get_json(&app, &format!("/context/runs/{run_id}/governance-evidence")).await;
        assert_eq!(status, StatusCode::OK, "evidence export readable");
        let package = evidence
            .get("evidence_package")
            .cloned()
            .unwrap_or(Value::Null);
        assert_eq!(
            package
                .pointer("/run/source_metadata/user_id")
                .and_then(Value::as_str),
            Some(finance.slack_user_id),
            "receipt carries the Slack requester identity"
        );
        let package_decisions = package
            .get("policy_decisions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(
            package_decisions.iter().any(|decision| {
                decision.get("decision_id").and_then(Value::as_str)
                    == Some(gate_decision.decision_id.as_str())
            }),
            "evidence package must correlate the persisted gate decision"
        );
        assert!(
            package
                .pointer("/audit/protected_events")
                .and_then(Value::as_array)
                .is_some_and(|events| {
                    events.iter().any(|event| {
                        event.get("event_type").and_then(Value::as_str)
                            == Some("approval.gate.approval_required")
                    })
                }),
            "evidence package must include the approval-required protected audit"
        );
        assert!(
            package
                .pointer("/final_outcome/slack_visible_response")
                .and_then(Value::as_str)
                .is_some_and(|response| !response.trim().is_empty()),
            "evidence package must expose the persisted Slack-visible response"
        );
    }

    journaler.abort();
    world.mock_task.abort();
}
