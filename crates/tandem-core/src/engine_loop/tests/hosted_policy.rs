use super::*;
use crate::engine_loop::tool_execution::EnginePreauthorizedDispatchPolicy;
use std::sync::atomic::AtomicBool;

struct DenyNestedPackBuilder;

impl ToolPolicyHook for DenyNestedPackBuilder {
    fn evaluate_tool(
        &self,
        _ctx: ToolPolicyContext,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<ToolPolicyDecision>> {
        Box::pin(async {
            Ok(ToolPolicyDecision {
                allowed: true,
                reason: None,
                policy_decision_id: None,
                dispatch_decision: None,
            })
        })
    }

    fn revalidate_dispatch(
        &self,
        context: tandem_tools::ToolDispatchPolicyContext,
    ) -> futures::future::BoxFuture<
        'static,
        anyhow::Result<Option<tandem_tools::ToolDispatchDecision>>,
    > {
        Box::pin(async move {
            Ok((context.canonical_tool.as_deref() == Some("pack_builder"))
                .then(|| tandem_tools::ToolDispatchDecision::deny("nested Pack Builder blocked")))
        })
    }
}

#[tokio::test]
async fn engine_dispatch_rechecks_canonical_batch_children() {
    use tandem_tools::{ToolDispatchPolicy, ToolDispatchPolicyContext, ToolDispatchPolicyOutcome};

    let policy = EnginePreauthorizedDispatchPolicy {
        decision: tandem_tools::ToolDispatchDecision::allow(),
        authority: Some(Arc::new(DenyNestedPackBuilder)),
    };
    let mut context = ToolDispatchPolicyContext {
        requested_tool: "batch".to_string(),
        canonical_tool: Some("batch".to_string()),
        args: json!({"tool_calls": []}),
        tenant_context: tandem_types::TenantContext::local_implicit(),
        verified_tenant_context: None,
        direct_loopback_http_request: false,
        source: tandem_tools::ToolDispatchSource::new("engine_loop")
            .session("session-a")
            .run("run-a"),
        scope_allowlist: vec!["batch".to_string(), "pack_builder".to_string()],
        schema: None,
    };
    assert_eq!(
        policy
            .evaluate(context.clone())
            .await
            .expect("parent policy")
            .outcome,
        ToolDispatchPolicyOutcome::Allowed
    );
    context.requested_tool = "functions.pack_builder".to_string();
    context.canonical_tool = Some("pack_builder".to_string());
    assert_eq!(
        policy
            .evaluate(context)
            .await
            .expect("child policy")
            .outcome,
        ToolDispatchPolicyOutcome::Denied
    );
}

struct MutableAuthority {
    revoked: Arc<AtomicBool>,
    initial_checks: Arc<AtomicUsize>,
}

struct HeldProviderStream {
    chunks: Mutex<Option<tokio::sync::mpsc::Receiver<StreamChunk>>>,
    cancel: Arc<Mutex<Option<CancellationToken>>>,
    terminal_chunk_seen: Option<Arc<AtomicBool>>,
}

#[async_trait]
impl Provider for HeldProviderStream {
    fn info(&self) -> tandem_types::ProviderInfo {
        tandem_types::ProviderInfo {
            id: "scripted-provider-stream".to_string(),
            name: "Held Provider Stream".to_string(),
            models: vec![tandem_types::ModelInfo {
                id: "scripted-model".to_string(),
                provider_id: "scripted-provider-stream".to_string(),
                display_name: "Scripted Model".to_string(),
                context_window: 8192,
            }],
        }
    }

    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        Ok("unused".to_string())
    }

    async fn stream(
        &self,
        _messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        _sampling: SamplingParams,
        cancel: CancellationToken,
    ) -> anyhow::Result<
        std::pin::Pin<Box<dyn futures::Stream<Item = anyhow::Result<StreamChunk>> + Send>>,
    > {
        *self.cancel.lock().expect("cancel lock") = Some(cancel);
        let chunks = self.chunks.lock().expect("chunks lock").take().unwrap();
        let terminal_chunk_seen = self.terminal_chunk_seen.clone();
        Ok(Box::pin(futures::stream::unfold(
            (chunks, terminal_chunk_seen),
            |(mut receiver, terminal_chunk_seen)| async move {
                receiver.recv().await.map(|chunk| {
                    if matches!(&chunk, StreamChunk::Done { .. }) {
                        if let Some(seen) = &terminal_chunk_seen {
                            seen.store(true, Ordering::SeqCst);
                        }
                    }
                    (Ok(chunk), (receiver, terminal_chunk_seen))
                })
            },
        )))
    }
}

impl ToolPolicyHook for MutableAuthority {
    fn evaluate_tool(
        &self,
        _ctx: ToolPolicyContext,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<ToolPolicyDecision>> {
        self.initial_checks.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Ok(ToolPolicyDecision {
                allowed: true,
                reason: None,
                policy_decision_id: None,
                dispatch_decision: None,
            })
        })
    }

    fn revalidate_session(
        &self,
        _verified: Option<tandem_types::VerifiedTenantContext>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<()>> {
        let revoked = self.revoked.clone();
        Box::pin(async move {
            anyhow::ensure!(!revoked.load(Ordering::SeqCst), "hosted membership revoked");
            Ok(())
        })
    }
}

struct CountedTool(Arc<AtomicUsize>);

#[async_trait]
impl Tool for CountedTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("counted_tool", "Synthetic effect counter", json!({}))
    }
    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(ToolResult {
            output: "effect executed".into(),
            metadata: json!({}),
        })
    }
}

#[tokio::test]
async fn hosted_policy_revocation_during_permission_wait_prevents_effect() {
    let temp = tempfile::tempdir().unwrap();
    let provider = Arc::new(SamplingCaptureProvider {
        captured: Arc::new(Mutex::new(None)),
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(temp.path(), provider).await;
    let engine = Arc::new(engine);
    let session = Session::new(
        Some("revocation during approval".into()),
        Some(temp.path().display().to_string()),
    );
    let session_id = session.id.clone();
    storage.save_session(session).await.unwrap();
    let effects = Arc::new(AtomicUsize::new(0));
    engine
        .tools
        .register_tool(
            "counted_tool".into(),
            Arc::new(CountedTool(effects.clone())),
        )
        .await;
    let revoked = Arc::new(AtomicBool::new(false));
    let initial_checks = Arc::new(AtomicUsize::new(0));
    engine
        .set_tool_policy_hook(Arc::new(MutableAuthority {
            revoked: revoked.clone(),
            initial_checks: initial_checks.clone(),
        }))
        .await;
    let mut events = bus.subscribe();
    let task_engine = engine.clone();
    let task = tokio::spawn(async move {
        task_engine
            .execute_tool_with_permission(
                &session_id,
                "message-1",
                None,
                "counted_tool".into(),
                json!({}),
                Some("call-1".into()),
                None,
                "test approved effect",
                false,
                None,
                CancellationToken::new(),
            )
            .await
    });
    let permission_id = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.event_type == "permission.asked" {
                break event.properties["requestID"].as_str().unwrap().to_string();
            }
        }
    })
    .await
    .expect("permission barrier reached");
    assert_eq!(initial_checks.load(Ordering::SeqCst), 1);
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    revoked.store(true, Ordering::SeqCst);
    assert!(engine.permissions.reply(&permission_id, "once").await);
    let error = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("revoked"));
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    assert_eq!(
        initial_checks.load(Ordering::SeqCst),
        1,
        "final check must not repeat approval-producing policy"
    );
}

#[tokio::test]
async fn hosted_policy_revocation_prevents_tool_free_provider_dispatch() {
    let temp = tempfile::tempdir().unwrap();
    let captured = Arc::new(Mutex::new(None));
    let provider = Arc::new(PostToolCaptureProvider {
        captured: captured.clone(),
    });
    let (engine, _, storage) = engine_loop_with_scripted_provider(temp.path(), provider).await;
    let session = Session::new(Some("revoked synthesis".into()), None);
    let session_id = session.id.clone();
    let _boundary_mode =
        ScopedDataBoundaryConfigOverride::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", None);
    storage.save_session(session).await.unwrap();
    engine
        .set_tool_policy_hook(Arc::new(MutableAuthority {
            revoked: Arc::new(AtomicBool::new(true)),
            initial_checks: Arc::new(AtomicUsize::new(0)),
        }))
        .await;
    let agent = engine.agents.get(None).await;
    let error = engine
        .generate_final_narrative_without_tools(
            &session_id,
            None,
            &agent,
            Some("scripted-provider-stream"),
            Some("scripted-model"),
            Default::default(),
            CancellationToken::new(),
            &["synthetic tool output".into()],
        )
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("revoked"),
        "expected revoked authority before tool-free dispatch, got: {error:#}"
    );
    assert!(captured.lock().unwrap().is_none());
}

#[tokio::test]
async fn hosted_policy_revocation_during_provider_stream_stops_output() {
    let temp = tempfile::tempdir().unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(4);
    let provider_cancel = Arc::new(Mutex::new(None));
    let provider = Arc::new(HeldProviderStream {
        chunks: Mutex::new(Some(receiver)),
        cancel: provider_cancel.clone(),
        terminal_chunk_seen: None,
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(temp.path(), provider).await;
    let engine = Arc::new(engine);
    let mut session = Session::new(Some("revocation during provider stream".into()), None);
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    let _boundary_mode =
        ScopedDataBoundaryConfigOverride::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", None);
    storage.save_session(session).await.unwrap();
    let revoked = Arc::new(AtomicBool::new(false));
    engine
        .set_tool_policy_hook(Arc::new(MutableAuthority {
            revoked: revoked.clone(),
            initial_checks: Arc::new(AtomicUsize::new(0)),
        }))
        .await;
    let mut events = bus.subscribe();
    let task_engine = engine.clone();
    let task_session_id = session_id.clone();
    let task = tokio::spawn(async move {
        task_engine
            .run_prompt_async(
                task_session_id,
                SendMessageRequest {
                    parts: vec![MessagePartInput::Text {
                        text: "stream a response".to_string(),
                    }],
                    model: Some(scripted_model()),
                    agent: None,
                    tool_mode: Some(ToolMode::None),
                    tool_allowlist: None,
                    strict_kb_grounding: None,
                    context_mode: None,
                    write_required: None,
                    prewrite_requirements: None,
                    sampling: Default::default(),
                },
            )
            .await
    });
    sender
        .send(StreamChunk::TextDelta("before revocation".to_string()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.event_type == "message.part.updated"
                && event.properties["delta"] == "before revocation"
            {
                break;
            }
        }
    })
    .await
    .expect("first stream delta reached the event bus");
    revoked.store(true, Ordering::SeqCst);
    sender
        .send(StreamChunk::TextDelta("after revocation".to_string()))
        .await
        .unwrap();
    let _ = sender
        .send(StreamChunk::Done {
            finish_reason: "stop".to_string(),
            usage: None,
        })
        .await;

    let error = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("revoked stream terminates")
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("revoked"));
    assert!(provider_cancel
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .is_cancelled());
    while let Ok(event) = events.try_recv() {
        assert_ne!(
            event.properties.get("delta"),
            Some(&json!("after revocation"))
        );
    }
    let session = storage.get_session(&session_id).await.unwrap();
    assert!(!session
        .messages
        .iter()
        .any(|message| matches!(message.role, MessageRole::Assistant)));
}

#[tokio::test]
async fn hosted_policy_revocation_stops_idle_provider_stream() {
    let temp = tempfile::tempdir().unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    let provider_cancel = Arc::new(Mutex::new(None));
    let provider = Arc::new(HeldProviderStream {
        chunks: Mutex::new(Some(receiver)),
        cancel: provider_cancel.clone(),
        terminal_chunk_seen: None,
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(temp.path(), provider).await;
    let engine = Arc::new(engine);
    let mut session = Session::new(Some("idle provider stream".into()), None);
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    let _boundary_mode =
        ScopedDataBoundaryConfigOverride::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", None);
    storage.save_session(session).await.unwrap();
    let revoked = Arc::new(AtomicBool::new(false));
    engine
        .set_tool_policy_hook(Arc::new(MutableAuthority {
            revoked: revoked.clone(),
            initial_checks: Arc::new(AtomicUsize::new(0)),
        }))
        .await;
    let mut events = bus.subscribe();
    let task_engine = engine.clone();
    let task_session_id = session_id.clone();
    let task = tokio::spawn(async move {
        task_engine
            .run_prompt_async(
                task_session_id,
                SendMessageRequest {
                    parts: vec![MessagePartInput::Text {
                        text: "wait for a response".to_string(),
                    }],
                    model: Some(scripted_model()),
                    agent: None,
                    tool_mode: Some(ToolMode::None),
                    tool_allowlist: None,
                    strict_kb_grounding: None,
                    context_mode: None,
                    write_required: None,
                    prewrite_requirements: None,
                    sampling: Default::default(),
                },
            )
            .await
    });
    sender
        .send(StreamChunk::TextDelta("before idle".to_string()))
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.event_type == "message.part.updated"
                && event.properties["delta"] == "before idle"
            {
                break;
            }
        }
    })
    .await
    .expect("first stream delta reached the event bus");
    revoked.store(true, Ordering::SeqCst);

    let error = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .expect("revocation stops an idle stream before the 90-second idle timeout")
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("revoked"));
    assert!(provider_cancel
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .is_cancelled());
    let session = storage.get_session(&session_id).await.unwrap();
    assert!(!session
        .messages
        .iter()
        .any(|message| matches!(message.role, MessageRole::Assistant)));
}

#[tokio::test]
async fn hosted_policy_valid_during_provider_stream_preserves_completion() {
    let temp = tempfile::tempdir().unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    let provider = Arc::new(HeldProviderStream {
        chunks: Mutex::new(Some(receiver)),
        cancel: Arc::new(Mutex::new(None)),
        terminal_chunk_seen: None,
    });
    let (engine, _, storage) = engine_loop_with_scripted_provider(temp.path(), provider).await;
    let mut session = Session::new(Some("authorized provider stream".into()), None);
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    let _boundary_mode =
        ScopedDataBoundaryConfigOverride::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", None);
    storage.save_session(session).await.unwrap();
    engine
        .set_tool_policy_hook(Arc::new(MutableAuthority {
            revoked: Arc::new(AtomicBool::new(false)),
            initial_checks: Arc::new(AtomicUsize::new(0)),
        }))
        .await;
    sender
        .send(StreamChunk::TextDelta("authorized answer".to_string()))
        .await
        .unwrap();
    sender
        .send(StreamChunk::Done {
            finish_reason: "stop".to_string(),
            usage: None,
        })
        .await
        .unwrap();
    engine
        .run_prompt_async(
            session_id.clone(),
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "answer normally".to_string(),
                }],
                model: Some(scripted_model()),
                agent: None,
                tool_mode: Some(ToolMode::None),
                tool_allowlist: None,
                strict_kb_grounding: None,
                context_mode: None,
                write_required: None,
                prewrite_requirements: None,
                sampling: Default::default(),
            },
        )
        .await
        .expect("valid hosted authority retains provider completion");
    let session = storage.get_session(&session_id).await.unwrap();
    assert!(session.messages.iter().any(|message| {
        matches!(message.role, MessageRole::Assistant)
            && message.parts.iter().any(|part| {
                matches!(part, MessagePart::Text { text } if text.contains("authorized answer"))
            })
    }));
}

#[tokio::test]
async fn hosted_policy_revocation_stops_post_tool_narrative_stream() {
    let temp = tempfile::tempdir().unwrap();
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    let provider_cancel = Arc::new(Mutex::new(None));
    let provider = Arc::new(HeldProviderStream {
        chunks: Mutex::new(Some(receiver)),
        cancel: provider_cancel.clone(),
        terminal_chunk_seen: None,
    });
    let (engine, _, storage) = engine_loop_with_scripted_provider(temp.path(), provider).await;
    let engine = Arc::new(engine);
    let session = Session::new(Some("post-tool narrative revocation".into()), None);
    let session_id = session.id.clone();
    let _boundary_mode =
        ScopedDataBoundaryConfigOverride::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", None);
    storage.save_session(session).await.unwrap();
    let revoked = Arc::new(AtomicBool::new(false));
    engine
        .set_tool_policy_hook(Arc::new(MutableAuthority {
            revoked: revoked.clone(),
            initial_checks: Arc::new(AtomicUsize::new(0)),
        }))
        .await;
    let active_agent = engine.agents.get(None).await;
    let task_engine = engine.clone();
    let task_session_id = session_id.clone();
    let task = tokio::spawn(async move {
        task_engine
            .generate_final_narrative_without_tools(
                &task_session_id,
                None,
                &active_agent,
                Some("scripted-provider-stream"),
                Some("scripted-model"),
                Default::default(),
                CancellationToken::new(),
                &["synthetic tool output".to_string()],
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while provider_cancel.lock().unwrap().is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("provider stream was created");
    revoked.store(true, Ordering::SeqCst);
    sender
        .send(StreamChunk::TextDelta("revoked narrative".to_string()))
        .await
        .unwrap();
    let _ = sender
        .send(StreamChunk::Done {
            finish_reason: "stop".to_string(),
            usage: None,
        })
        .await;
    let error = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("narrative stream terminates")
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("revoked"));
    assert!(provider_cancel
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .is_cancelled());
}

struct FinalAppendAuthority {
    revoked: Arc<AtomicBool>,
    terminal_chunk_seen: Arc<AtomicBool>,
    post_terminal_checks: AtomicUsize,
    final_precheck: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    committed_identities: Arc<Mutex<Vec<Option<tandem_types::VerifiedTenantContext>>>>,
    commit_observer: Option<FinalAppendCommitObserver>,
}

struct FinalAppendCommitObserver {
    database_path: std::path::PathBuf,
    session_id: String,
    events: Mutex<tokio::sync::broadcast::Receiver<EngineEvent>>,
    observed: Arc<AtomicBool>,
}

impl ToolPolicyHook for FinalAppendAuthority {
    fn evaluate_tool(
        &self,
        _ctx: ToolPolicyContext,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<ToolPolicyDecision>> {
        Box::pin(async {
            Ok(ToolPolicyDecision {
                allowed: true,
                reason: None,
                policy_decision_id: None,
                dispatch_decision: None,
            })
        })
    }

    fn revalidate_session(
        &self,
        _verified: Option<tandem_types::VerifiedTenantContext>,
    ) -> futures::future::BoxFuture<'static, anyhow::Result<()>> {
        // Capture the actual successful decision before notifying the test.
        // Done is checked once by the provider poll and then by the final
        // transcript precheck. There is no await between this notification
        // and the returned ready decision.
        let allowed = !self.revoked.load(Ordering::SeqCst);
        if self.terminal_chunk_seen.load(Ordering::SeqCst)
            && self.post_terminal_checks.fetch_add(1, Ordering::SeqCst) == 1
        {
            self.final_precheck
                .lock()
                .expect("final precheck signal")
                .take()
                .expect("one final precheck")
                .send(())
                .expect("final precheck receiver");
        }
        Box::pin(async move {
            anyhow::ensure!(allowed, "hosted final transcript authority revoked");
            Ok(())
        })
    }

    fn with_session_commit_authority(
        &self,
        verified: Option<tandem_types::VerifiedTenantContext>,
        commit: &mut dyn FnMut() -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        self.committed_identities
            .lock()
            .expect("captured commit identities")
            .push(verified);
        anyhow::ensure!(
            !self.revoked.load(Ordering::SeqCst),
            "hosted final transcript authority revoked"
        );
        let result = commit();
        if result.is_ok() {
            if let Some(observer) = &self.commit_observer {
                let connection = rusqlite::Connection::open(&observer.database_path)
                    .expect("independent commit observer");
                let assistants: usize = connection.query_row(
                    "SELECT COUNT(*) FROM session_messages WHERE session_id = ?1 AND role = 'assistant'",
                    [&observer.session_id],
                    |row| row.get(0),
                ).expect("observe committed assistant");
                assert_eq!(
                    assistants, 1,
                    "authority continuation includes the actual durable commit"
                );
                let mut receiver = observer.events.lock().expect("commit event observer");
                let published = std::iter::from_fn(|| receiver.try_recv().ok()).collect::<Vec<_>>();
                for event_type in ["message.part.updated", "session.updated", "session.status"] {
                    let count = published
                        .iter()
                        .filter(|event| {
                            event.event_type == event_type
                                && event.properties["sessionID"] == observer.session_id
                                && if event_type == "message.part.updated" {
                                    event.properties.get("delta").is_none()
                                } else {
                                    event.properties["status"] == "idle"
                                }
                        })
                        .count();
                    assert_eq!(
                        count, 1,
                        "{event_type} success must publish inside authority continuation"
                    );
                }
                observer.observed.store(true, Ordering::SeqCst);
            }
        }
        result
    }
}

#[derive(Clone, Copy)]
enum FinalAppendAuthorityMode {
    RevokeAfterPrecheck,
    Current,
    Standalone,
    ObservePublication,
    AssistantInsertFailure,
    RenewStoredAuthority,
}

struct FinalAppendWriterResult {
    result: anyhow::Result<()>,
    session: Session,
    events: Vec<EngineEvent>,
    provider_cancelled: bool,
    committed_identities: Vec<Option<tandem_types::VerifiedTenantContext>>,
    publication_observed: bool,
}

fn final_append_identity(
    tenant: tandem_types::TenantContext,
    assertion_id: &str,
    policy_version: u64,
) -> tandem_types::VerifiedTenantContext {
    // Use the same claims constructor as storage authority-update tests.
    let mut claims = tandem_types::TenantContextAssertionClaims::new_v1(
        "test-issuer",
        "test-audience",
        1,
        u64::MAX,
        assertion_id,
        tenant,
        tandem_types::HumanActor::tandem_user("final-authority-user"),
        tandem_types::AuthorityChain::from_request(
            tandem_types::RequestPrincipal::authenticated_user(
                "final-authority-user",
                "test-issuer",
            ),
        ),
        Vec::new(),
    );
    claims.policy_version = Some(policy_version);
    claims.into()
}

async fn run_final_append_writer_wait(mode: FinalAppendAuthorityMode) -> FinalAppendWriterResult {
    let temp = tempfile::tempdir().expect("final transcript directory");
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    let terminal_chunk_seen = Arc::new(AtomicBool::new(false));
    let provider_cancel = Arc::new(Mutex::new(None));
    let provider = Arc::new(HeldProviderStream {
        chunks: Mutex::new(Some(receiver)),
        cancel: provider_cancel.clone(),
        terminal_chunk_seen: Some(terminal_chunk_seen.clone()),
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(temp.path(), provider).await;
    let engine = Arc::new(engine);
    let mut session = Session::new(Some("final transcript writer wait".into()), None);
    session.model = Some(scripted_model());
    if matches!(mode, FinalAppendAuthorityMode::RenewStoredAuthority) {
        session.tenant_context = tandem_types::TenantContext::explicit_user_workspace(
            "final-authority-org",
            "final-authority-workspace",
            Some("final-authority-deployment".into()),
            "final-authority-user",
        );
        session.verified_tenant_context = Some(final_append_identity(
            session.tenant_context.clone(),
            "original-final-authority",
            1,
        ));
    }
    let session_id = session.id.clone();
    let _boundary_mode =
        ScopedDataBoundaryConfigOverride::set(&session_id, "TANDEM_DATA_BOUNDARY_MODE", None);
    storage.save_session(session).await.expect("save session");
    let revoked = Arc::new(AtomicBool::new(false));
    let committed_identities = Arc::new(Mutex::new(Vec::new()));
    let publication_observed = Arc::new(AtomicBool::new(false));
    let (precheck_tx, precheck_rx) = tokio::sync::oneshot::channel();
    if !matches!(mode, FinalAppendAuthorityMode::Standalone) {
        engine
            .set_tool_policy_hook(Arc::new(FinalAppendAuthority {
                revoked: revoked.clone(),
                terminal_chunk_seen,
                post_terminal_checks: AtomicUsize::new(0),
                final_precheck: Mutex::new(Some(precheck_tx)),
                committed_identities: committed_identities.clone(),
                commit_observer: matches!(mode, FinalAppendAuthorityMode::ObservePublication).then(
                    || FinalAppendCommitObserver {
                        database_path: temp.path().join("sessions.sqlite3"),
                        session_id: session_id.clone(),
                        events: Mutex::new(bus.subscribe()),
                        observed: publication_observed.clone(),
                    },
                ),
            }))
            .await;
    }
    let mut events = bus.subscribe();
    let task_engine = engine.clone();
    let task_session_id = session_id.clone();
    let task = tokio::spawn(async move {
        task_engine
            .run_prompt_async(
                task_session_id,
                SendMessageRequest {
                    parts: vec![MessagePartInput::Text {
                        text: "Answer the synthetic question.".to_string(),
                    }],
                    model: Some(scripted_model()),
                    agent: None,
                    tool_mode: Some(ToolMode::None),
                    tool_allowlist: None,
                    strict_kb_grounding: None,
                    context_mode: None,
                    write_required: None,
                    prewrite_requirements: None,
                    sampling: Default::default(),
                },
            )
            .await
    });
    sender
        .send(StreamChunk::TextDelta("held final answer".to_string()))
        .await
        .expect("send final text");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = events.recv().await.expect("stream event");
            if event.event_type == "message.part.updated"
                && event.properties["delta"] == "held final answer"
            {
                break;
            }
        }
    })
    .await
    .expect("provider output reached the real event bus");

    if matches!(mode, FinalAppendAuthorityMode::RenewStoredAuthority) {
        // The actual provider has already consumed the original prompt's
        // authority. Renew only the stored header before allowing it to finish.
        let tenant = storage
            .get_session(&session_id)
            .await
            .expect("captured session")
            .tenant_context;
        let renewed = final_append_identity(tenant.clone(), "renewed-final-authority", 2);
        assert!(storage
            .update_session_authority(&session_id, tenant, Some(renewed))
            .await
            .expect("renew stored authority"));
    }

    // Initial session/user writes are finished. An independent SQLite
    // connection now excludes the final append's IMMEDIATE transaction.
    let writer = rusqlite::Connection::open(temp.path().join("sessions.sqlite3"))
        .expect("open competing SQLite writer");
    if matches!(mode, FinalAppendAuthorityMode::AssistantInsertFailure) {
        writer
            .execute_batch(
                "CREATE TRIGGER reject_final_assistant BEFORE INSERT ON session_messages
             WHEN NEW.role = 'assistant'
             BEGIN SELECT RAISE(ABORT, 'synthetic assistant insert failure'); END;",
            )
            .expect("inject an actual SQLite assistant insert failure");
    }
    writer
        .execute_batch("BEGIN IMMEDIATE")
        .expect("hold final transcript writer");
    sender
        .send(StreamChunk::Done {
            finish_reason: "stop".to_string(),
            usage: None,
        })
        .await
        .expect("finish provider stream");
    if matches!(mode, FinalAppendAuthorityMode::Standalone) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if events
                    .recv()
                    .await
                    .expect("provider completion event")
                    .event_type
                    == "provider.usage"
                {
                    break;
                }
            }
        })
        .await
        .expect("standalone provider completed");
    } else {
        tokio::time::timeout(Duration::from_secs(5), precheck_rx)
            .await
            .expect("final async authority precheck reached")
            .expect("final async authority precheck succeeded");
    }
    // These cases use a current-thread runtime. The engine cannot yield
    // between signaling its successful final precheck (or publishing usage
    // with no hook) and queuing the blocking final append. The competing
    // transaction remains held; no timing sleep substitutes for this barrier.
    assert!(
        !task.is_finished(),
        "final append must wait for the actual SQLite writer"
    );
    if matches!(mode, FinalAppendAuthorityMode::RevokeAfterPrecheck) {
        revoked.store(true, Ordering::SeqCst);
    }
    writer
        .execute_batch("COMMIT")
        .expect("release SQLite writer");
    let result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .expect("final append finishes after writer release")
        .expect("prompt task joins");
    let session = storage
        .get_session(&session_id)
        .await
        .expect("stored session");
    let events = std::iter::from_fn(|| events.try_recv().ok()).collect();
    let provider_cancelled = provider_cancel
        .lock()
        .expect("provider cancellation")
        .as_ref()
        .expect("provider stream was dispatched")
        .is_cancelled();
    let committed_identities = committed_identities
        .lock()
        .expect("commit identities")
        .clone();
    let publication_observed = publication_observed.load(Ordering::SeqCst);
    FinalAppendWriterResult {
        result,
        session,
        events,
        provider_cancelled,
        committed_identities,
        publication_observed,
    }
}

fn assert_final_append_success(outcome: FinalAppendWriterResult) {
    outcome
        .result
        .expect("current authority completes final append");
    let assistants = outcome
        .session
        .messages
        .iter()
        .filter(|message| matches!(message.role, MessageRole::Assistant))
        .collect::<Vec<_>>();
    assert_eq!(
        assistants.len(),
        1,
        "one final assistant message is durable"
    );
    assert!(assistants[0]
        .parts
        .iter()
        .any(|part| { matches!(part, MessagePart::Text { text } if text == "held final answer") }));
    assert!(
        outcome.events.iter().any(|event| {
            event.event_type == "message.part.updated" && event.properties.get("delta").is_none()
        }),
        "final assistant success event must be published"
    );
    assert!(
        outcome.events.iter().any(|event| {
            event.event_type == "session.status" && event.properties["status"] == "idle"
        }),
        "successful prompt remains usable"
    );
    assert!(!outcome.provider_cancelled);
}

#[tokio::test(flavor = "current_thread")]
async fn hosted_final_assistant_writer_wait_rechecks_revoked_authority() {
    let outcome = run_final_append_writer_wait(FinalAppendAuthorityMode::RevokeAfterPrecheck).await;
    let assistants = outcome
        .session
        .messages
        .iter()
        .filter(|message| matches!(message.role, MessageRole::Assistant))
        .collect::<Vec<_>>();
    assert!(
        assistants.is_empty(),
        "revoked final assistant committed: {assistants:?}"
    );
    let error = outcome.result.expect_err("revoked final append must fail");
    assert!(error
        .to_string()
        .contains("hosted final transcript authority revoked"));
    assert!(
        !outcome.events.iter().any(|event| {
            event.event_type == "message.part.updated" && event.properties.get("delta").is_none()
        }),
        "revoked final assistant success must not be published"
    );
    assert!(
        !outcome.events.iter().any(|event| {
            matches!(
                event.event_type.as_str(),
                "session.status" | "session.updated"
            ) && event.properties["status"] == "idle"
        }),
        "revoked final append cannot report successful idle completion"
    );
    assert!(
        outcome.provider_cancelled,
        "revoked prompt must cancel its provider authority"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hosted_final_assistant_writer_wait_preserves_current_authority() {
    assert_final_append_success(
        run_final_append_writer_wait(FinalAppendAuthorityMode::Current).await,
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hosted_final_assistant_writer_wait_preserves_standalone_without_hook() {
    assert_final_append_success(
        run_final_append_writer_wait(FinalAppendAuthorityMode::Standalone).await,
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hosted_final_assistant_writer_wait_keeps_commit_and_success_events_inside_authority() {
    let outcome = run_final_append_writer_wait(FinalAppendAuthorityMode::ObservePublication).await;
    assert!(
        outcome.publication_observed,
        "commit hook observed durable assistant and all final events before return"
    );
    assert_final_append_success(outcome);
}

#[tokio::test(flavor = "current_thread")]
async fn hosted_final_assistant_writer_wait_insert_failure_has_no_final_success() {
    let outcome =
        run_final_append_writer_wait(FinalAppendAuthorityMode::AssistantInsertFailure).await;
    let error = outcome
        .result
        .expect_err("SQLite assistant insert must fail");
    assert!(error
        .to_string()
        .contains("synthetic assistant insert failure"));
    assert!(!outcome
        .session
        .messages
        .iter()
        .any(|message| matches!(message.role, MessageRole::Assistant)));
    assert!(
        !outcome.events.iter().any(|event| {
            event.event_type == "message.part.updated" && event.properties.get("delta").is_none()
        }),
        "failed SQLite commit cannot publish final assistant success"
    );
    assert!(
        !outcome.events.iter().any(|event| {
            matches!(
                event.event_type.as_str(),
                "session.updated" | "session.status"
            ) && event.properties["status"] == "idle"
        }),
        "failed SQLite commit cannot publish successful idle completion"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hosted_final_assistant_writer_wait_retains_original_prompt_authority_after_renewal() {
    let outcome =
        run_final_append_writer_wait(FinalAppendAuthorityMode::RenewStoredAuthority).await;
    assert_eq!(
        outcome.committed_identities.len(),
        1,
        "one final guarded commit"
    );
    let captured = outcome.committed_identities[0]
        .as_ref()
        .expect("original verified prompt authority");
    assert_eq!(captured.assertion_id, "original-final-authority");
    assert_eq!(captured.policy_version, Some(1));
    let stored = outcome
        .session
        .verified_tenant_context
        .as_ref()
        .expect("renewed stored authority");
    assert_eq!(stored.assertion_id, "renewed-final-authority");
    assert_eq!(stored.policy_version, Some(2));
    assert_final_append_success(outcome);
}
