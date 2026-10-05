// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use crate::agent_teams::ServerToolPolicyHook;
use async_trait::async_trait;
use futures::{future::BoxFuture, Stream};
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc, Mutex,
};
use std::time::Duration;
use tandem_core::{
    EngineLoop, ScopedDataBoundaryConfigOverride, Storage, ToolPolicyContext, ToolPolicyDecision,
    ToolPolicyHook,
};
use tandem_providers::{ChatMessage, Provider, StreamChunk};
use tandem_types::{
    EngineEvent, MessagePart, MessagePartInput, MessageRole, ModelInfo, ModelSpec, ProviderInfo,
    SamplingParams, SendMessageRequest, Session, ToolMode, ToolSchema,
};
use tokio::sync::{broadcast, oneshot};
use tokio_util::sync::CancellationToken;

#[path = "hosted_policy_planning_commit_tests.rs"]
mod planning_commit;

const PROVIDER: &str = "held-final-assistant";
const FINAL_TEXT: &str = "authorized final assistant response";

struct HeldFinalProvider {
    chunks: Mutex<Option<tokio::sync::mpsc::Receiver<StreamChunk>>>,
    terminal_seen: Arc<AtomicBool>,
    cancellation: Arc<Mutex<Option<CancellationToken>>>,
}

#[async_trait]
impl Provider for HeldFinalProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: PROVIDER.into(),
            name: "Held final assistant provider".into(),
            models: vec![ModelInfo {
                id: "held-model".into(),
                provider_id: PROVIDER.into(),
                display_name: "Held model".into(),
                context_window: 8192,
            }],
        }
    }

    async fn complete(&self, _: &str, _: Option<&str>) -> anyhow::Result<String> {
        anyhow::bail!("the final assistant test requires the held stream")
    }

    async fn stream(
        &self,
        _: Vec<ChatMessage>,
        _: Option<&str>,
        _: ToolMode,
        _: Option<Vec<ToolSchema>>,
        _: SamplingParams,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        *self.cancellation.lock().unwrap() = Some(cancel);
        let receiver = self.chunks.lock().unwrap().take().unwrap();
        let terminal_seen = self.terminal_seen.clone();
        Ok(Box::pin(futures::stream::unfold(
            (receiver, terminal_seen),
            |(mut receiver, terminal_seen)| async move {
                receiver.recv().await.map(|chunk| {
                    if matches!(chunk, StreamChunk::Done { .. }) {
                        terminal_seen.store(true, Ordering::SeqCst);
                    }
                    (Ok(chunk), (receiver, terminal_seen))
                })
            },
        )))
    }
}

struct CommitGate {
    entered: oneshot::Sender<(bool, Vec<EngineEvent>)>,
    release: mpsc::Receiver<()>,
}

/// Decorate the real host hook only to observe its final precheck and pause
/// after the core-provided commit/publication continuation has finished.
struct ObservedServerAuthority {
    server: ServerToolPolicyHook,
    state: AppState,
    terminal_seen: Arc<AtomicBool>,
    post_terminal_checks: AtomicUsize,
    final_precheck: Mutex<Option<oneshot::Sender<VerifiedTenantContext>>>,
    commits: Arc<Mutex<Vec<Option<VerifiedTenantContext>>>>,
    evaluations: Arc<AtomicUsize>,
    gate: Mutex<Option<CommitGate>>,
    events: Mutex<broadcast::Receiver<EngineEvent>>,
}

impl ToolPolicyHook for ObservedServerAuthority {
    fn revalidate_session(
        &self,
        verified: Option<VerifiedTenantContext>,
    ) -> BoxFuture<'static, anyhow::Result<()>> {
        let check = self.server.revalidate_session(verified.clone());
        // The provider poll checks Done once; the next check is the final
        // transcript precheck. Signal only after the real host allows it.
        let signal = if self.terminal_seen.load(Ordering::SeqCst)
            && self.post_terminal_checks.fetch_add(1, Ordering::SeqCst) == 1
        {
            self.final_precheck.lock().unwrap().take()
        } else {
            None
        };
        Box::pin(async move {
            check.await?;
            if let Some(signal) = signal {
                let _ = signal.send(verified.expect("hosted prompt identity"));
            }
            Ok(())
        })
    }

    fn with_session_commit_authority(
        &self,
        verified: Option<VerifiedTenantContext>,
        commit: &mut dyn FnMut() -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.commits.lock().unwrap().push(verified.clone());
        let mut observed_commit = || {
            commit()?;
            if let Some(gate) = self.gate.lock().unwrap().take() {
                let held = self
                    .state
                    .enterprise
                    .hosted_policy
                    .publication_write_blocked_for_test();
                let mut events = self.events.lock().unwrap();
                let mut published = Vec::new();
                while let Ok(event) = events.try_recv() {
                    published.push(event);
                }
                let _ = gate.entered.send((held, published));
                // Dropping the test's release sender also releases this
                // native worker if an assertion fails in the waiting task.
                let _ = gate.release.recv();
            }
            Ok(())
        };
        self.server
            .with_session_commit_authority(verified, &mut observed_commit)
    }

    fn revalidate_dispatch(
        &self,
        context: tandem_tools::ToolDispatchPolicyContext,
    ) -> BoxFuture<'static, anyhow::Result<Option<tandem_tools::ToolDispatchDecision>>> {
        self.server.revalidate_dispatch(context)
    }

    fn evaluate_tool(
        &self,
        context: ToolPolicyContext,
    ) -> BoxFuture<'static, anyhow::Result<ToolPolicyDecision>> {
        self.evaluations.fetch_add(1, Ordering::SeqCst);
        self.server.evaluate_tool(context)
    }
}

struct Fixture {
    state: AppState,
    engine: EngineLoop,
    storage: Arc<Storage>,
    directory: tempfile::TempDir,
    session_id: String,
    verified: VerifiedTenantContext,
    chunks: tokio::sync::mpsc::Sender<StreamChunk>,
    terminal_seen: Arc<AtomicBool>,
    cancellation: Arc<Mutex<Option<CancellationToken>>>,
    _boundary: ScopedDataBoundaryConfigOverride,
}

impl Fixture {
    async fn new() -> Self {
        let state = crate::test_support::test_state().await;
        let directory = tempfile::tempdir().unwrap();
        let policy_path = directory.path().join("policy.json");
        write_input(&policy_path, &policy_json(4, crate::now_ms(), true));
        state
            .enterprise
            .hosted_policy
            .configure_test_source("org-a", "dep-a", policy_path);
        state.reload_hosted_policy().await.unwrap();
        let verified = identity(4);
        let storage = Arc::new(
            Storage::new(directory.path().join("sessions"))
                .await
                .unwrap(),
        );
        let (chunks, receiver) = tokio::sync::mpsc::channel(2);
        let terminal_seen = Arc::new(AtomicBool::new(false));
        let cancellation = Arc::new(Mutex::new(None));
        state
            .providers
            .replace_for_test(
                vec![Arc::new(HeldFinalProvider {
                    chunks: Mutex::new(Some(receiver)),
                    terminal_seen: terminal_seen.clone(),
                    cancellation: cancellation.clone(),
                })],
                Some(PROVIDER.into()),
            )
            .await;
        let engine = EngineLoop::new(
            storage.clone(),
            state.event_bus.clone(),
            state.providers.clone(),
            state.plugins.clone(),
            state.agents.clone(),
            state.permissions.clone(),
            state.tools.clone(),
            state.cancellations.clone(),
            state.host_runtime_context.clone(),
        );
        let mut session = Session::new(
            Some("hosted final assistant writer".into()),
            Some(directory.path().display().to_string()),
        );
        session.model = Some(model());
        session.tenant_context = verified.tenant_context.clone();
        session.verified_tenant_context = Some(verified.clone());
        let session_id = session.id.clone();
        let boundary =
            ScopedDataBoundaryConfigOverride::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", None);
        storage.save_session(session).await.unwrap();
        Self {
            state,
            engine,
            storage,
            directory,
            session_id,
            verified,
            chunks,
            terminal_seen,
            cancellation,
            _boundary: boundary,
        }
    }

    async fn observe(
        &self,
        gate: Option<CommitGate>,
    ) -> (
        oneshot::Receiver<VerifiedTenantContext>,
        Arc<Mutex<Vec<Option<VerifiedTenantContext>>>>,
        Arc<AtomicUsize>,
    ) {
        let (signal, precheck) = oneshot::channel();
        let commits = Arc::new(Mutex::new(Vec::new()));
        let evaluations = Arc::new(AtomicUsize::new(0));
        self.engine
            .set_tool_policy_hook(Arc::new(ObservedServerAuthority {
                server: ServerToolPolicyHook::new(self.state.clone()),
                state: self.state.clone(),
                terminal_seen: self.terminal_seen.clone(),
                post_terminal_checks: AtomicUsize::new(0),
                final_precheck: Mutex::new(Some(signal)),
                commits: commits.clone(),
                evaluations: evaluations.clone(),
                gate: Mutex::new(gate),
                events: Mutex::new(self.state.event_bus.subscribe()),
            }))
            .await;
        (precheck, commits, evaluations)
    }

    fn start(&self) -> tokio::task::JoinHandle<anyhow::Result<()>> {
        let engine = self.engine.clone();
        let session_id = self.session_id.clone();
        tokio::spawn(async move {
            engine
                .run_prompt_async_with_execution_context(
                    session_id,
                    SendMessageRequest {
                        parts: vec![MessagePartInput::Text {
                            text: "Answer the synthetic question.".into(),
                        }],
                        model: Some(model()),
                        agent: None,
                        tool_mode: Some(ToolMode::None),
                        tool_allowlist: None,
                        strict_kb_grounding: None,
                        context_mode: None,
                        write_required: None,
                        prewrite_requirements: None,
                        sampling: Default::default(),
                    },
                    None,
                    Some("hosted-final-run".into()),
                    Vec::new(),
                )
                .await
        })
    }

    async fn stream_text(&self, events: &mut broadcast::Receiver<EngineEvent>) {
        self.chunks
            .send(StreamChunk::TextDelta(FINAL_TEXT.into()))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = events.recv().await.unwrap();
                if event.event_type == "message.part.updated"
                    && event.properties["delta"] == FINAL_TEXT
                {
                    break;
                }
            }
        })
        .await
        .expect("held provider text reached the actual engine event bus");
    }

    async fn finish_stream(&self) {
        self.chunks
            .send(StreamChunk::Done {
                finish_reason: "stop".into(),
                usage: None,
            })
            .await
            .unwrap();
    }
}

fn model() -> ModelSpec {
    ModelSpec {
        provider_id: PROVIDER.into(),
        model_id: "held-model".into(),
    }
}

fn final_events(events: &[EngineEvent]) -> Vec<&EngineEvent> {
    events
        .iter()
        .filter(|event| {
            (event.event_type == "message.part.updated"
                && event.properties.get("delta").is_none()
                && event.properties["part"]["text"] == FINAL_TEXT)
                || (matches!(
                    event.event_type.as_str(),
                    "session.updated" | "session.status"
                ) && event.properties["status"] == "idle")
        })
        .collect()
}

#[tokio::test]
async fn hosted_final_assistant_rechecks_real_policy_after_sqlite_writer_wait() {
    for revoked in [false, true] {
        let fixture = Fixture::new().await;
        let (precheck, commits, evaluations) = fixture.observe(None).await;
        let mut events = fixture.state.event_bus.subscribe();
        let task = fixture.start();
        fixture.stream_text(&mut events).await;
        let writer =
            rusqlite::Connection::open(fixture.directory.path().join("sessions/sessions.sqlite3"))
                .unwrap();
        writer.execute_batch("BEGIN IMMEDIATE").unwrap();
        fixture.finish_stream().await;
        let checked = tokio::time::timeout(Duration::from_secs(5), precheck)
            .await
            .unwrap()
            .expect("real host allowed the final async precheck");
        assert_eq!(checked.assertion_id, fixture.verified.assertion_id);
        assert_eq!(checked.policy_version, Some(4));
        // On this current-thread runtime, the successful ready precheck
        // queues the final native append before the receiver is resumed.
        assert!(!task.is_finished());
        assert!(commits.lock().unwrap().is_empty());
        if revoked {
            write_input(
                &fixture.directory.path().join("policy.json"),
                &policy_json(5, crate::now_ms(), false),
            );
            fixture.state.reload_hosted_policy().await.unwrap();
        }
        writer.execute_batch("COMMIT").unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        let session = fixture
            .storage
            .get_session(&fixture.session_id)
            .await
            .unwrap();
        let assistants: Vec<_> = session
            .messages
            .iter()
            .filter(|message| matches!(message.role, MessageRole::Assistant))
            .collect();
        let mut published = Vec::new();
        while let Ok(event) = events.try_recv() {
            published.push(event);
        }
        let commits = commits.lock().unwrap();
        assert_eq!(commits.len(), 1);
        let committed_identity = commits[0].as_ref().unwrap();
        assert_eq!(
            committed_identity.assertion_id,
            fixture.verified.assertion_id
        );
        assert_eq!(committed_identity.policy_version, Some(4));
        assert_eq!(evaluations.load(Ordering::SeqCst), 0);
        if revoked {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("hosted_identity_policy_revision_changed"));
            assert!(assistants.is_empty());
            assert!(final_events(&published).is_empty());
            assert!(published.iter().any(|event| {
                event.event_type == "session.status" && event.properties["status"] == "failed"
            }));
            assert!(fixture
                .cancellation
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .is_cancelled());
        } else {
            result.unwrap();
            assert_eq!(assistants.len(), 1);
            assert!(assistants[0]
                .parts
                .iter()
                .any(|part| { matches!(part, MessagePart::Text { text } if text == FINAL_TEXT) }));
            assert_eq!(final_events(&published).len(), 3);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hosted_final_assistant_holds_policy_through_events_and_cancelled_caller() {
    let fixture = Fixture::new().await;
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (_, commits, evaluations) = fixture
        .observe(Some(CommitGate {
            entered: entered_tx,
            release: release_rx,
        }))
        .await;
    let mut events = fixture.state.event_bus.subscribe();
    let task = fixture.start();
    fixture.stream_text(&mut events).await;
    fixture.finish_stream().await;
    let (held, published) = tokio::time::timeout(Duration::from_secs(5), entered_rx)
        .await
        .unwrap()
        .expect("core commit and final event enqueue completed under real host authority");
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert!(
        held,
        "policy snapshot must cover SQL commit and all success events"
    );
    assert_eq!(final_events(&published).len(), 3);
    let session = fixture
        .storage
        .get_session(&fixture.session_id)
        .await
        .unwrap();
    assert_eq!(
        session
            .messages
            .iter()
            .filter(|message| matches!(message.role, MessageRole::Assistant))
            .count(),
        1,
        "the final assistant is durable before its final events are observed"
    );
    let replacement = HostedPolicyBundle::from_json(&policy_json(5, crate::now_ms(), true))
        .unwrap()
        .validate("org-a", "dep-a", crate::now_ms(), None)
        .unwrap();
    let publication_state = fixture.state.clone();
    let (attempted_tx, attempted_rx) = oneshot::channel();
    let publisher = tokio::task::spawn_blocking(move || {
        let runtime = &publication_state.enterprise.hosted_policy;
        let blocked = runtime.snapshot.try_write().is_err();
        let _ = attempted_tx.send(blocked);
        *runtime.snapshot.write().unwrap() = Some(Arc::new(replacement));
    });
    assert!(
        tokio::time::timeout(Duration::from_secs(5), attempted_rx)
            .await
            .unwrap()
            .unwrap(),
        "cancelling the awaiting caller must retain the native policy guard"
    );
    assert!(!publisher.is_finished());
    release_tx.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), publisher)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        fixture
            .state
            .enterprise
            .hosted_policy
            .revision()
            .unwrap()
            .unwrap()
            .version,
        5
    );
    assert_eq!(commits.lock().unwrap().len(), 1);
    assert_eq!(evaluations.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn hosted_final_commit_preserves_local_and_fails_closed_without_policy_or_identity() {
    let state = crate::test_support::test_state().await;
    let hook = ServerToolPolicyHook::new(state.clone());
    let calls = AtomicUsize::new(0);
    let mut commit = || {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    };
    hook.with_session_commit_authority(None, &mut commit)
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    state.enterprise.hosted_policy.configure_test_source(
        "org-a",
        "dep-a",
        PathBuf::from("unused-policy.json"),
    );
    assert!(hook
        .with_session_commit_authority(Some(identity(4)), &mut commit)
        .is_err());
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(
            HostedPolicyBundle::from_json(&policy_json(4, crate::now_ms(), true)).unwrap(),
        )
        .unwrap();
    assert!(hook
        .with_session_commit_authority(None, &mut commit)
        .is_err());
    let mut expired = identity(4);
    expired.expires_at_ms = crate::now_ms();
    assert!(hook
        .with_session_commit_authority(Some(expired), &mut commit)
        .is_err());
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "denied authority must never invoke commit"
    );
}
