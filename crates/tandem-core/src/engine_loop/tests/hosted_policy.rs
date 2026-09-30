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
        Ok(Box::pin(futures::stream::unfold(
            chunks,
            |mut receiver| async move { receiver.recv().await.map(|chunk| (Ok(chunk), receiver)) },
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
    assert!(error.to_string().contains("revoked"));
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
