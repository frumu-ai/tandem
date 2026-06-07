use super::*;
use async_trait::async_trait;
use futures::stream;
use futures::Stream;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tandem_providers::{AppConfig, Provider};
use tandem_tools::Tool;
use tandem_types::ToolResult;

struct ScriptedProviderStream {
    calls: Arc<AtomicUsize>,
    mode: ScriptedProviderStreamMode,
}

#[derive(Clone, Copy)]
enum ScriptedProviderStreamMode {
    DecodeThenSuccess,
    IdleThenSuccess,
    AuthFailure,
    EndlessToolCalls,
}

struct FailingTool;
struct LoopingTool;

#[async_trait]
impl Tool for FailingTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new("failing_tool", "fails for testing", json!({}))
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        anyhow::bail!("transient connector failure")
    }
}

#[async_trait]
impl Tool for LoopingTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "loop_tool",
            "returns output for iteration-budget tests",
            json!({}),
        )
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            output: "loop tool produced output".to_string(),
            metadata: json!({}),
        })
    }
}

#[async_trait]
impl Provider for ScriptedProviderStream {
    fn info(&self) -> tandem_types::ProviderInfo {
        tandem_types::ProviderInfo {
            id: "scripted-provider-stream".to_string(),
            name: "Scripted Provider Stream".to_string(),
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
        Ok("complete fallback".to_string())
    }

    async fn stream(
        &self,
        _messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        _sampling: tandem_types::SamplingParams,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        match self.mode {
            ScriptedProviderStreamMode::DecodeThenSuccess if call == 0 => {
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::TextDelta("partial text".to_string())),
                    Err(anyhow::anyhow!("error decoding response body")),
                ])))
            }
            ScriptedProviderStreamMode::DecodeThenSuccess => Ok(Box::pin(stream::iter(vec![
                Ok(StreamChunk::TextDelta("final answer".to_string())),
                Ok(StreamChunk::Done {
                    finish_reason: "stop".to_string(),
                    usage: None,
                }),
            ]))),
            ScriptedProviderStreamMode::IdleThenSuccess if call == 0 => {
                Ok(Box::pin(stream::pending()))
            }
            ScriptedProviderStreamMode::IdleThenSuccess => Ok(Box::pin(stream::iter(vec![
                Ok(StreamChunk::TextDelta(
                    "final answer after idle retry".to_string(),
                )),
                Ok(StreamChunk::Done {
                    finish_reason: "stop".to_string(),
                    usage: None,
                }),
            ]))),
            ScriptedProviderStreamMode::AuthFailure => {
                anyhow::bail!("authentication failed for scripted provider")
            }
            ScriptedProviderStreamMode::EndlessToolCalls => {
                let call_id = format!("loop-tool-call-{call}");
                Ok(Box::pin(stream::iter(vec![
                    Ok(StreamChunk::ToolCallStart {
                        id: call_id.clone(),
                        name: "loop_tool".to_string(),
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
                ])))
            }
        }
    }
}

async fn engine_loop_with_scripted_provider(
    base: &std::path::Path,
    provider: Arc<dyn Provider>,
) -> (EngineLoop, EventBus, Arc<Storage>) {
    let storage = Arc::new(Storage::new(base).await.expect("storage"));
    let bus = EventBus::new();
    let providers = ProviderRegistry::new(AppConfig::default());
    providers
        .replace_for_test(vec![provider], Some("scripted-provider-stream".to_string()))
        .await;
    let plugins = PluginRegistry::new(base).await.expect("plugins");
    let agents = AgentRegistry::new(base).await.expect("agents");
    let permissions = PermissionManager::new(bus.clone());
    let tools = ToolRegistry::new();
    tools
        .register_tool("loop_tool".to_string(), Arc::new(LoopingTool))
        .await;
    let cancellations = CancellationRegistry::new();
    let host_runtime_context = HostRuntimeContext {
        os: HostOs::Linux,
        arch: std::env::consts::ARCH.to_string(),
        shell_family: ShellFamily::Posix,
        path_style: PathStyle::Posix,
    };
    let engine = EngineLoop::new(
        storage.clone(),
        bus.clone(),
        providers,
        plugins,
        agents,
        permissions,
        tools,
        cancellations,
        host_runtime_context,
    );
    (engine, bus, storage)
}

fn scripted_model() -> ModelSpec {
    ModelSpec {
        provider_id: "scripted-provider-stream".to_string(),
        model_id: "scripted-model".to_string(),
    }
}

/// Provider that records the sampling parameters it receives, then emits a
/// trivial successful completion. Used to assert sampling reaches the adapter
/// boundary.
struct SamplingCaptureProvider {
    captured: Arc<std::sync::Mutex<Option<tandem_types::SamplingParams>>>,
}

#[async_trait]
impl Provider for SamplingCaptureProvider {
    fn info(&self) -> tandem_types::ProviderInfo {
        tandem_types::ProviderInfo {
            id: "scripted-provider-stream".to_string(),
            name: "Sampling Capture".to_string(),
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
        Ok("complete fallback".to_string())
    }

    async fn stream(
        &self,
        _messages: Vec<ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        sampling: tandem_types::SamplingParams,
        _cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        *self.captured.lock().unwrap() = Some(sampling);
        Ok(Box::pin(stream::iter(vec![
            Ok(StreamChunk::TextDelta("ok".to_string())),
            Ok(StreamChunk::Done {
                finish_reason: "stop".to_string(),
                usage: None,
            }),
        ])))
    }
}

#[tokio::test]
async fn session_sampling_default_reaches_provider() {
    let base =
        std::env::temp_dir().join(format!("engine-loop-sampling-default-{}", Uuid::new_v4()));
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider = Arc::new(SamplingCaptureProvider {
        captured: captured.clone(),
    });
    let (engine, _bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let mut session = Session::new(Some("sampling default".to_string()), Some(".".to_string()));
    session.model = Some(scripted_model());
    session.sampling = tandem_types::SamplingParams {
        temperature: Some(0.1),
        top_p: None,
        max_tokens: Some(2048),
    };
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    engine
        .run_prompt_async(
            session_id,
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "answer once".to_string(),
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
        .expect("prompt runs");

    let seen = captured
        .lock()
        .unwrap()
        .expect("provider received sampling");
    assert_eq!(seen.temperature, Some(0.1));
    assert_eq!(seen.max_tokens, Some(2048));
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn per_prompt_sampling_overrides_session_default() {
    let base =
        std::env::temp_dir().join(format!("engine-loop-sampling-override-{}", Uuid::new_v4()));
    let captured = Arc::new(std::sync::Mutex::new(None));
    let provider = Arc::new(SamplingCaptureProvider {
        captured: captured.clone(),
    });
    let (engine, _bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let mut session = Session::new(Some("sampling override".to_string()), Some(".".to_string()));
    session.model = Some(scripted_model());
    session.sampling = tandem_types::SamplingParams {
        temperature: Some(0.1),
        top_p: Some(0.8),
        max_tokens: None,
    };
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    engine
        .run_prompt_async(
            session_id,
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "answer once".to_string(),
                }],
                model: Some(scripted_model()),
                agent: None,
                tool_mode: Some(ToolMode::None),
                tool_allowlist: None,
                strict_kb_grounding: None,
                context_mode: None,
                write_required: None,
                prewrite_requirements: None,
                sampling: tandem_types::SamplingParams {
                    temperature: Some(0.9),
                    top_p: None,
                    max_tokens: Some(512),
                },
            },
        )
        .await
        .expect("prompt runs");

    let seen = captured
        .lock()
        .unwrap()
        .expect("provider received sampling");
    // Per-prompt temperature/max_tokens win; session top_p fills the gap.
    assert_eq!(seen.temperature, Some(0.9));
    assert_eq!(seen.top_p, Some(0.8));
    assert_eq!(seen.max_tokens, Some(512));
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn nonfatal_tool_execution_error_is_recoverable_model_output() {
    let output = recoverable_tool_execution_error_output(
        "mcp.notion.create_page",
        "HTTP 500: transient upstream failure",
    );

    assert!(!tool_execution_error_is_prompt_fatal(
        "HTTP 500: transient upstream failure",
        false
    ));
    assert!(output.contains("Tool `mcp.notion.create_page` failed during execution"));
    assert!(output.contains("recoverable in the current turn"));
    assert!(output.contains("HTTP 500: transient upstream failure"));
}

#[tokio::test]
async fn nonfatal_tool_execution_error_does_not_abort_prompt_execution() {
    let base = std::env::temp_dir().join(format!("engine-loop-test-{}", Uuid::new_v4()));
    let storage = Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(
        Some("s".to_string()),
        Some(base.to_string_lossy().to_string()),
    );
    let session_id = session.id.clone();
    storage
        .save_session(session.clone())
        .await
        .expect("save session");
    let bus = EventBus::new();
    let providers = ProviderRegistry::new(AppConfig::default());
    let plugins = PluginRegistry::new(&base).await.expect("plugins");
    let agents = AgentRegistry::new(&base).await.expect("agents");
    let permissions = PermissionManager::new(bus.clone());
    let tools = ToolRegistry::new();
    tools
        .register_tool("failing_tool".to_string(), Arc::new(FailingTool))
        .await;
    let cancellations = CancellationRegistry::new();
    let host_runtime_context = HostRuntimeContext {
        os: HostOs::Linux,
        arch: std::env::consts::ARCH.to_string(),
        shell_family: ShellFamily::Posix,
        path_style: PathStyle::Posix,
    };
    let engine = EngineLoop::new(
        storage,
        bus,
        providers,
        plugins,
        agents,
        permissions,
        tools,
        cancellations,
        host_runtime_context,
    );
    engine
        .set_session_allowed_tools(&session_id, vec!["failing_tool".to_string()])
        .await;
    engine
        .set_session_auto_approve_permissions(&session_id, true)
        .await;

    let output = engine
        .execute_tool_with_permission(
            &session_id,
            "message-1",
            "failing_tool".to_string(),
            json!({}),
            Some("tool-call-1".to_string()),
            None,
            "use the failing tool",
            false,
            None,
            CancellationToken::new(),
        )
        .await
        .expect("recoverable tool errors should not abort the prompt");

    let output = output.expect("recoverable tool error should be surfaced to the model");
    assert!(output.contains("Tool `failing_tool` failed during execution"));
    assert!(output.contains("transient connector failure"));
    assert!(output.contains("recoverable in the current turn"));
}

#[tokio::test]
async fn write_required_auto_approve_denial_fails_loudly() {
    let base = std::env::temp_dir().join(format!("engine-loop-c10-deny-{}", Uuid::new_v4()));
    let storage = Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(
        Some("c10 auto approve denial".to_string()),
        Some(base.to_string_lossy().to_string()),
    );
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    let bus = EventBus::new();
    let providers = ProviderRegistry::new(AppConfig::default());
    let plugins = PluginRegistry::new(&base).await.expect("plugins");
    let agents = AgentRegistry::new(&base).await.expect("agents");
    let permissions = PermissionManager::new(bus.clone());
    let tools = ToolRegistry::new();
    tools
        .register_tool("failing_tool".to_string(), Arc::new(FailingTool))
        .await;
    let cancellations = CancellationRegistry::new();
    let host_runtime_context = HostRuntimeContext {
        os: HostOs::Linux,
        arch: std::env::consts::ARCH.to_string(),
        shell_family: ShellFamily::Posix,
        path_style: PathStyle::Posix,
    };
    let engine = EngineLoop::new(
        storage,
        bus,
        providers,
        plugins,
        agents,
        permissions,
        tools,
        cancellations,
        host_runtime_context,
    );
    engine
        .set_session_auto_approve_permissions(&session_id, true)
        .await;

    let error = engine
        .execute_tool_with_permission(
            &session_id,
            "message-1",
            "failing_tool".to_string(),
            json!({}),
            Some("tool-call-1".to_string()),
            None,
            "use the failing tool",
            true,
            None,
            CancellationToken::new(),
        )
        .await
        .expect_err("write-required auto-approve denial should fail loudly");

    assert!(error.to_string().contains("Permission auto-approve denied"));
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn write_required_permission_timeout_fails_loudly() {
    let _guard = env_test_lock();
    unsafe {
        std::env::set_var("TANDEM_PERMISSION_WAIT_TIMEOUT_MS", "1");
    }
    let base = std::env::temp_dir().join(format!("engine-loop-c10-timeout-{}", Uuid::new_v4()));
    let storage = Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(
        Some("c10 permission timeout".to_string()),
        Some(base.to_string_lossy().to_string()),
    );
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    let bus = EventBus::new();
    let providers = ProviderRegistry::new(AppConfig::default());
    let plugins = PluginRegistry::new(&base).await.expect("plugins");
    let agents = AgentRegistry::new(&base).await.expect("agents");
    let permissions = PermissionManager::new(bus.clone());
    let tools = ToolRegistry::new();
    tools
        .register_tool("failing_tool".to_string(), Arc::new(FailingTool))
        .await;
    let cancellations = CancellationRegistry::new();
    let host_runtime_context = HostRuntimeContext {
        os: HostOs::Linux,
        arch: std::env::consts::ARCH.to_string(),
        shell_family: ShellFamily::Posix,
        path_style: PathStyle::Posix,
    };
    let engine = EngineLoop::new(
        storage,
        bus,
        providers,
        plugins,
        agents,
        permissions,
        tools,
        cancellations,
        host_runtime_context,
    );

    let error = engine
        .execute_tool_with_permission(
            &session_id,
            "message-1",
            "failing_tool".to_string(),
            json!({}),
            Some("tool-call-1".to_string()),
            None,
            "use the failing tool",
            true,
            None,
            CancellationToken::new(),
        )
        .await
        .expect_err("write-required permission timeout should fail loudly");

    assert!(error.to_string().contains("Permission request"));
    assert!(error.to_string().contains("timed out"));
    unsafe {
        std::env::remove_var("TANDEM_PERMISSION_WAIT_TIMEOUT_MS");
    }
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn cancellation_and_shutdown_tool_errors_remain_prompt_fatal() {
    assert!(tool_execution_error_is_prompt_fatal(
        "tool future observed operation cancelled",
        false
    ));
    assert!(tool_execution_error_is_prompt_fatal(
        "runtime not ready",
        false
    ));
    assert!(tool_execution_error_is_prompt_fatal("ordinary error", true));
}

#[tokio::test]
async fn todo_updated_event_is_normalized() {
    let base = std::env::temp_dir().join(format!("engine-loop-test-{}", Uuid::new_v4()));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = tandem_types::Session::new(Some("s".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    emit_tool_side_events(
        storage.clone(),
        &bus,
        ToolSideEventContext {
            session_id: &session_id,
            message_id: "m1",
            tool: "todo_write",
            args: &json!({"todos":[{"content":"ship parity"}]}),
            metadata: &json!({"todos":[{"content":"ship parity"}]}),
            workspace_root: Some("."),
            effective_cwd: Some("."),
        },
    )
    .await;

    let event = rx.recv().await.expect("event");
    assert_eq!(event.event_type, "todo.updated");
    let todos = event
        .properties
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(todos.len(), 1);
    assert!(todos[0].get("id").and_then(|v| v.as_str()).is_some());
    assert_eq!(
        todos[0].get("content").and_then(|v| v.as_str()),
        Some("ship parity")
    );
    assert!(todos[0].get("status").and_then(|v| v.as_str()).is_some());
}

#[tokio::test]
async fn channel_pinned_workspace_blocks_file_read_outside_pin() {
    let base = std::env::temp_dir().join(format!("engine-loop-test-{}", Uuid::new_v4()));
    let provider = Arc::new(ScriptedProviderStream {
        calls: Arc::new(AtomicUsize::new(0)),
        mode: ScriptedProviderStreamMode::DecodeThenSuccess,
    });
    let (engine, _bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let acme = base.join("workspaces/acme");
    let other = base.join("workspaces/other");
    std::fs::create_dir_all(&acme).expect("acme workspace");
    std::fs::create_dir_all(&other).expect("other workspace");
    let other_file = other.join("secret.txt");
    std::fs::write(&other_file, "nope").expect("other file");

    let mut session = tandem_types::Session::new(
        Some("slack channel".to_string()),
        Some(other.to_string_lossy().to_string()),
    );
    session.source_kind = Some("channel".to_string());
    session.workspace_root = Some(other.to_string_lossy().to_string());
    session.pinned_workspace_id = Some(acme.to_string_lossy().to_string());
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let violation = engine
        .workspace_sandbox_violation(
            &session_id,
            "read",
            &json!({ "path": other_file.to_string_lossy().to_string() }),
        )
        .await
        .expect("workspace scope violation");
    assert!(violation.contains("ToolDenied { reason: WorkspaceScope }"));
}

#[tokio::test]
async fn provider_stream_idle_timeout_retries_current_iteration() {
    let _guard = env_test_lock();
    std::env::set_var("TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS", "1");
    std::env::set_var("TANDEM_PROVIDER_STREAM_DECODE_RETRY_ATTEMPTS", "1");

    let base = std::env::temp_dir().join(format!(
        "engine-loop-provider-stream-idle-retry-{}",
        Uuid::new_v4()
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(ScriptedProviderStream {
        calls: calls.clone(),
        mode: ScriptedProviderStreamMode::IdleThenSuccess,
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let mut session = Session::new(
        Some("provider stream idle retry".to_string()),
        Some(".".to_string()),
    );
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    let mut rx = bus.subscribe();

    engine
        .run_prompt_async(
            session_id.clone(),
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "answer once".to_string(),
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
        .expect("prompt should recover from idle timeout");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let session = storage.get_session(&session_id).await.expect("session");
    let assistant_text = session
        .messages
        .iter()
        .rev()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            MessagePart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    assert!(assistant_text.contains("final answer after idle retry"));

    let mut saw_retry = false;
    while let Ok(event) = rx.try_recv() {
        if event.event_type == "provider.call.iteration.retry" {
            saw_retry = true;
            assert_eq!(
                event.properties.get("retry").and_then(Value::as_u64),
                Some(1)
            );
            assert!(event
                .properties
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("provider stream idle timeout")));
        }
    }
    assert!(saw_retry);

    std::env::remove_var("TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS");
    std::env::remove_var("TANDEM_PROVIDER_STREAM_DECODE_RETRY_ATTEMPTS");
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn provider_stream_decode_error_retries_current_iteration() {
    let base = std::env::temp_dir().join(format!(
        "engine-loop-provider-stream-retry-{}",
        Uuid::new_v4()
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(ScriptedProviderStream {
        calls: calls.clone(),
        mode: ScriptedProviderStreamMode::DecodeThenSuccess,
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let mut session = Session::new(
        Some("provider stream retry".to_string()),
        Some(".".to_string()),
    );
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    let mut rx = bus.subscribe();

    engine
        .run_prompt_async(
            session_id.clone(),
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "answer once".to_string(),
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
        .expect("prompt should recover");

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let session = storage.get_session(&session_id).await.expect("session");
    let assistant_text = session
        .messages
        .iter()
        .rev()
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            MessagePart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(assistant_text.contains("final answer"));
    assert!(!assistant_text.contains("partial text"));

    let mut saw_retry = false;
    while let Ok(event) = rx.try_recv() {
        if event.event_type == "provider.call.iteration.retry" {
            saw_retry = true;
            assert_eq!(
                event.properties.get("retry").and_then(Value::as_u64),
                Some(1)
            );
            assert!(event
                .properties
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("error decoding response body")));
        }
    }
    assert!(saw_retry);

    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn iteration_budget_exhaustion_fails_run_without_idle_completion() {
    let _guard = env_test_lock();
    std::env::set_var("TANDEM_MAX_TOOL_ITERATIONS", "1");

    let base =
        std::env::temp_dir().join(format!("engine-loop-iteration-budget-{}", Uuid::new_v4()));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(ScriptedProviderStream {
        calls: calls.clone(),
        mode: ScriptedProviderStreamMode::EndlessToolCalls,
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let mut session = Session::new(
        Some("provider stream iteration budget".to_string()),
        Some(".".to_string()),
    );
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    engine
        .set_session_allowed_tools(&session_id, vec!["loop_tool".to_string()])
        .await;
    engine
        .set_session_auto_approve_permissions(&session_id, true)
        .await;
    let mut rx = bus.subscribe();

    let result = engine
        .run_prompt_async(
            session_id.clone(),
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "keep calling the loop tool".to_string(),
                }],
                model: Some(scripted_model()),
                agent: None,
                tool_mode: Some(ToolMode::Auto),
                tool_allowlist: Some(vec!["loop_tool".to_string()]),
                strict_kb_grounding: None,
                context_mode: None,
                write_required: None,
                prewrite_requirements: None,
                sampling: Default::default(),
            },
        )
        .await;

    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let session = storage.get_session(&session_id).await.expect("session");
    let assistant_text = session
        .messages
        .iter()
        .filter(|message| matches!(message.role, MessageRole::Assistant))
        .flat_map(|message| message.parts.iter())
        .filter_map(|part| match part {
            MessagePart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(assistant_text.trim().is_empty());

    let mut saw_budget_event = false;
    let mut saw_failed_status = false;
    while let Ok(event) = rx.try_recv() {
        if event.event_type == "provider.call.iteration.budget_exhausted" {
            saw_budget_event = true;
            assert_eq!(
                event
                    .properties
                    .get("maxIterations")
                    .and_then(Value::as_u64),
                Some(1)
            );
            assert!(event
                .properties
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("iteration budget")));
        }
        if event.event_type == "session.status"
            && event.properties.get("status").and_then(Value::as_str) == Some("failed")
        {
            saw_failed_status = true;
            assert!(event
                .properties
                .get("error")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("iteration budget")));
        }
    }
    assert!(saw_budget_event);
    assert!(saw_failed_status);

    std::env::remove_var("TANDEM_MAX_TOOL_ITERATIONS");
    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn provider_stream_auth_error_does_not_retry() {
    let base = std::env::temp_dir().join(format!(
        "engine-loop-provider-stream-auth-{}",
        Uuid::new_v4()
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(ScriptedProviderStream {
        calls: calls.clone(),
        mode: ScriptedProviderStreamMode::AuthFailure,
    });
    let (engine, bus, storage) = engine_loop_with_scripted_provider(&base, provider).await;
    let mut session = Session::new(
        Some("provider stream auth".to_string()),
        Some(".".to_string()),
    );
    session.model = Some(scripted_model());
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");
    let mut rx = bus.subscribe();

    let result = engine
        .run_prompt_async(
            session_id,
            SendMessageRequest {
                parts: vec![MessagePartInput::Text {
                    text: "answer once".to_string(),
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
        .await;

    assert!(result.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    while let Ok(event) = rx.try_recv() {
        assert_ne!(event.event_type, "provider.call.iteration.retry");
    }

    let _ = std::fs::remove_dir_all(base);
}

#[tokio::test]
async fn question_asked_event_contains_tool_reference() {
    let base = std::env::temp_dir().join(format!("engine-loop-test-{}", Uuid::new_v4()));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = tandem_types::Session::new(Some("s".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let bus = EventBus::new();
    let mut rx = bus.subscribe();
    emit_tool_side_events(
        storage,
        &bus,
        ToolSideEventContext {
            session_id: &session_id,
            message_id: "msg-1",
            tool: "question",
            args: &json!({"questions":[{"header":"Topic","question":"Pick one","options":[{"label":"A","description":"d"}]}]}),
            metadata: &json!({"questions":[{"header":"Topic","question":"Pick one","options":[{"label":"A","description":"d"}]}]}),
            workspace_root: Some("."),
            effective_cwd: Some("."),
        },
    )
    .await;

    let event = rx.recv().await.expect("event");
    assert_eq!(event.event_type, "question.asked");
    assert_eq!(
        event
            .properties
            .get("sessionID")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        session_id
    );
    let tool = event
        .properties
        .get("tool")
        .cloned()
        .unwrap_or_else(|| json!({}));
    assert!(tool.get("callID").and_then(|v| v.as_str()).is_some());
    assert_eq!(
        tool.get("messageID").and_then(|v| v.as_str()),
        Some("msg-1")
    );
}

#[test]
fn compact_chat_history_keeps_recent_and_inserts_summary() {
    let mut messages = Vec::new();
    for i in 0..60 {
        messages.push(ChatMessage {
            role: "user".to_string(),
            content: format!("message-{i}"),
            attachments: Vec::new(),
        });
    }
    let compacted = compact_chat_history(messages, ChatHistoryProfile::Standard);
    assert!(compacted.len() <= 41);
    assert_eq!(compacted[0].role, "system");
    assert!(compacted[0].content.contains("history compacted"));
    assert!(compacted.iter().any(|m| m.content.contains("message-59")));
}

#[tokio::test]
async fn load_chat_history_preserves_tool_args_and_error_context() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-load-chat-history-error-{}",
        uuid::Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("chat history".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let message = Message::new(
        MessageRole::User,
        vec![
            MessagePart::Text {
                text: "build the page".to_string(),
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"game.html","content":"<html>draft</html>"}),
                result: None,
                error: Some("WRITE_ARGS_EMPTY_FROM_PROVIDER".to_string()),
            },
        ],
    );
    storage
        .append_message(&session_id, message)
        .await
        .expect("append message");

    let history = load_chat_history(storage, &session_id, ChatHistoryProfile::Standard).await;
    let content = history
        .iter()
        .find(|message| message.role == "user")
        .map(|message| message.content.clone())
        .unwrap_or_default();
    assert!(content.contains("build the page"));
    assert!(content.contains("Tool write"));
    assert!(content.contains(r#"args={"content":"<html>draft</html>","path":"game.html"}"#));
    assert!(content.contains("error=WRITE_ARGS_EMPTY_FROM_PROVIDER"));
}

#[tokio::test]
async fn load_chat_history_preserves_tool_args_and_result_context() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-load-chat-history-result-{}",
        uuid::Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("chat history".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let message = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "glob".to_string(),
            args: json!({"pattern":"src/**/*.rs"}),
            result: Some(json!({"output":"src/lib.rs\nsrc/main.rs"})),
            error: None,
        }],
    );
    storage
        .append_message(&session_id, message)
        .await
        .expect("append message");

    let history = load_chat_history(storage, &session_id, ChatHistoryProfile::Standard).await;
    let content = history
        .iter()
        .find(|message| message.role == "assistant")
        .map(|message| message.content.clone())
        .unwrap_or_default();
    assert!(content.contains("Tool glob"));
    assert!(content.contains(r#"args={"pattern":"src/**/*.rs"}"#));
    assert!(content.contains(r#"result={"output":"src/lib.rs\nsrc/main.rs"}"#));
}

#[tokio::test]
async fn load_chat_history_compacts_mcp_list_results() {
    let base = std::env::temp_dir().join(format!(
        "tandem-core-load-chat-history-mcp-list-{}",
        uuid::Uuid::new_v4()
    ));
    let storage = std::sync::Arc::new(Storage::new(&base).await.expect("storage"));
    let session = Session::new(Some("chat history".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    storage.save_session(session).await.expect("save session");

    let message = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "mcp_list".to_string(),
            args: json!({}),
            result: Some(json!({
                "connected_server_names": ["gmail"],
                "registered_tools": ["mcp.gmail.gmail_create_email_draft"],
                "servers": [{
                    "name": "gmail",
                    "registered_tools": ["mcp.gmail.gmail_send_draft"],
                    "verbose": "x".repeat(5000)
                }]
            })),
            error: None,
        }],
    );
    storage
        .append_message(&session_id, message)
        .await
        .expect("append message");

    let history = load_chat_history(storage, &session_id, ChatHistoryProfile::Standard).await;
    let content = history
        .iter()
        .find(|message| message.role == "assistant")
        .map(|message| message.content.clone())
        .unwrap_or_default();
    assert!(content.contains("mcp_list result compacted for chat history"));
    assert!(content.contains("mcp.gmail.gmail_create_email_draft"));
    assert!(content.contains("mcp.gmail.gmail_send_draft"));
    assert!(!content.contains(&"x".repeat(100)));
}

#[test]
fn extracts_todos_from_checklist_and_numbered_lines() {
    let input = r#"
Plan:
- [ ] Audit current implementation
- [ ] Add planner fallback
1. Add regression test coverage
"#;
    let todos = extract_todo_candidates_from_text(input);
    assert_eq!(todos.len(), 3);
    assert_eq!(
        todos[0].get("content").and_then(|v| v.as_str()),
        Some("Audit current implementation")
    );
}

#[test]
fn does_not_extract_todos_from_plain_prose_lines() {
    let input = r#"
I need more information to proceed.
Can you tell me the event size and budget?
Once I have that, I can provide a detailed plan.
"#;
    let todos = extract_todo_candidates_from_text(input);
    assert!(todos.is_empty());
}

#[test]
fn parses_wrapped_tool_call_from_markdown_response() {
    let input = r#"
Here is the tool call:
```json
{"tool_call":{"name":"todo_write","arguments":{"todos":[{"content":"a"}]}}}
```
"#;
    let parsed = parse_tool_invocation_from_response(input).expect("tool call");
    assert_eq!(parsed.0, "todo_write");
    assert!(parsed.1.get("todos").is_some());
}

#[test]
fn parses_top_level_name_args_tool_call() {
    let input = r#"{"name":"bash","args":{"command":"echo hi"}}"#;
    let parsed = parse_tool_invocation_from_response(input).expect("top-level tool call");
    assert_eq!(parsed.0, "bash");
    assert_eq!(
        parsed.1.get("command").and_then(|v| v.as_str()),
        Some("echo hi")
    );
}

#[test]
fn parses_function_style_todowrite_call() {
    let input = r#"Status: Completed
Call: todowrite(task_id=2, status="completed")"#;
    let parsed = parse_tool_invocation_from_response(input).expect("function-style tool call");
    assert_eq!(parsed.0, "todo_write");
    assert_eq!(parsed.1.get("task_id").and_then(|v| v.as_i64()), Some(2));
    assert_eq!(
        parsed.1.get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
}

#[test]
fn parses_multiple_function_style_todowrite_calls() {
    let input = r#"
Call: todowrite(task_id=2, status="completed")
Call: todowrite(task_id=3, status="in_progress")
"#;
    let parsed = parse_tool_invocations_from_response(input);
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].0, "todo_write");
    assert_eq!(parsed[0].1.get("task_id").and_then(|v| v.as_i64()), Some(2));
    assert_eq!(
        parsed[0].1.get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
    assert_eq!(parsed[1].1.get("task_id").and_then(|v| v.as_i64()), Some(3));
    assert_eq!(
        parsed[1].1.get("status").and_then(|v| v.as_str()),
        Some("in_progress")
    );
}

#[test]
fn applies_todo_status_update_from_task_id_args() {
    let current = vec![
        json!({"id":"todo-1","content":"a","status":"pending"}),
        json!({"id":"todo-2","content":"b","status":"pending"}),
        json!({"id":"todo-3","content":"c","status":"pending"}),
    ];
    let updated =
        apply_todo_updates_from_args(current, &json!({"task_id":2, "status":"completed"}))
            .expect("status update");
    assert_eq!(
        updated[1].get("status").and_then(|v| v.as_str()),
        Some("completed")
    );
}

#[test]
fn normalizes_todo_write_tasks_alias() {
    let normalized = normalize_todo_write_args(
        json!({"tasks":[{"title":"Book venue"},{"name":"Send invites"}]}),
        "",
    );
    let todos = normalized
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(todos.len(), 2);
    assert_eq!(
        todos[0].get("content").and_then(|v| v.as_str()),
        Some("Book venue")
    );
    assert_eq!(
        todos[1].get("content").and_then(|v| v.as_str()),
        Some("Send invites")
    );
}

#[test]
fn normalizes_todo_write_from_completion_when_args_empty() {
    let completion = "Plan:\n1. Secure venue\n2. Create playlist\n3. Send invites";
    let normalized = normalize_todo_write_args(json!({}), completion);
    let todos = normalized
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(todos.len(), 3);
    assert!(!is_empty_todo_write_args(&normalized));
}

#[test]
fn empty_todo_write_args_allows_status_updates() {
    let args = json!({"task_id": 2, "status":"completed"});
    assert!(!is_empty_todo_write_args(&args));
}

#[test]
fn streamed_websearch_args_fallback_to_query_string() {
    let parsed = parse_streamed_tool_args("websearch", "meaning of life");
    assert_eq!(
        parsed.get("query").and_then(|v| v.as_str()),
        Some("meaning of life")
    );
}

#[test]
fn parse_scalar_like_value_handles_single_quote_character_without_panicking() {
    assert_eq!(
        parse_scalar_like_value("\""),
        Value::String("\"".to_string())
    );
    assert_eq!(parse_scalar_like_value("'"), Value::String("'".to_string()));
}

#[test]
fn streamed_websearch_stringified_json_args_are_unwrapped() {
    let parsed = parse_streamed_tool_args("websearch", r#""donkey gestation period""#);
    assert_eq!(
        parsed.get("query").and_then(|v| v.as_str()),
        Some("donkey gestation period")
    );
}

#[test]
fn streamed_websearch_args_strip_arg_key_value_wrappers() {
    let parsed = parse_streamed_tool_args(
        "websearch",
        "query</arg_key><arg_value>taj card what is it benefits how to apply</arg_value>",
    );
    assert_eq!(
        parsed.get("query").and_then(|v| v.as_str()),
        Some("taj card what is it benefits how to apply")
    );
}

#[test]
fn normalize_tool_args_websearch_infers_from_user_text() {
    let normalized = normalize_tool_args("websearch", json!({}), "web search meaning of life", "");
    assert_eq!(
        normalized.args.get("query").and_then(|v| v.as_str()),
        Some("meaning of life")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_websearch_keeps_existing_query() {
    let normalized = normalize_tool_args(
        "websearch",
        json!({"query":"already set"}),
        "web search should not override",
        "",
    );
    assert_eq!(
        normalized.args.get("query").and_then(|v| v.as_str()),
        Some("already set")
    );
    assert_eq!(normalized.args_source, "provider_json");
    assert_eq!(normalized.args_integrity, "ok");
}

#[test]
fn normalize_tool_args_websearch_fails_when_unrecoverable() {
    let normalized = normalize_tool_args("websearch", json!({}), "search", "");
    assert!(normalized.query.is_none());
    assert!(normalized.missing_terminal);
    assert_eq!(normalized.args_source, "missing");
    assert_eq!(normalized.args_integrity, "empty");
}

#[test]
fn normalize_tool_args_webfetch_infers_url_from_user_prompt() {
    let normalized = normalize_tool_args(
        "webfetch",
        json!({}),
        "Please fetch `https://docs.tandem.ac/` in markdown mode",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("url").and_then(|v| v.as_str()),
        Some("https://docs.tandem.ac/")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_webfetch_recovers_nested_url_alias() {
    let normalized = normalize_tool_args(
        "webfetch",
        json!({"args":{"uri":"https://example.com/page"}}),
        "",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("url").and_then(|v| v.as_str()),
        Some("https://example.com/page")
    );
    assert_eq!(normalized.args_source, "provider_json");
}

#[test]
fn normalize_tool_args_webfetch_fails_when_url_unrecoverable() {
    let normalized = normalize_tool_args("webfetch", json!({}), "fetch the site", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("WEBFETCH_URL_MISSING")
    );
}

#[test]
fn normalize_tool_args_answer_how_to_infers_task_from_user_prompt() {
    let user_text = "what is tandem and how do i use it?";
    let normalized = normalize_tool_args("mcp.tandem_mcp.answer_how_to", json!({}), user_text, "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("task").and_then(|v| v.as_str()),
        Some(user_text)
    );
    assert_eq!(
        normalized
            .args
            .get("engine_version")
            .and_then(|v| v.as_str()),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_answer_how_to_keeps_existing_task() {
    let normalized = normalize_tool_args(
        "mcp.tandem_mcp.answer_how_to",
        json!({"task":"install tandem locally"}),
        "different user prompt",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("task").and_then(|v| v.as_str()),
        Some("install tandem locally")
    );
    assert_eq!(normalized.args_source, "provider_json");
    assert_eq!(normalized.args_integrity, "ok");
}

#[test]
fn normalize_tool_args_search_docs_infers_query_from_user_prompt() {
    let user_text = "https://docs.tandem.ac/start-here/";
    let normalized = normalize_tool_args("mcp.tandem_mcp.search_docs", json!({}), user_text, "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("query").and_then(|v| v.as_str()),
        Some(user_text)
    );
    assert_eq!(
        normalized
            .args
            .get("engine_version")
            .and_then(|v| v.as_str()),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_search_docs_keeps_existing_query() {
    let normalized = normalize_tool_args(
        "mcp.tandem_mcp.search_docs",
        json!({"query":"oauth setup"}),
        "different user prompt",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("query").and_then(|v| v.as_str()),
        Some("oauth setup")
    );
    assert_eq!(normalized.args_source, "provider_json");
    assert_eq!(normalized.args_integrity, "ok");
}

#[test]
fn normalize_tool_args_get_doc_infers_path_from_user_url() {
    let user_text = "https://docs.tandem.ac/start-here/";
    let normalized = normalize_tool_args("mcp.tandem_mcp.get_doc", json!({}), user_text, "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some(user_text)
    );
    assert_eq!(
        normalized
            .args
            .get("engine_version")
            .and_then(|v| v.as_str()),
        Some(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_tandem_docs_keeps_existing_engine_version() {
    let normalized = normalize_tool_args(
        "mcp.tandem_mcp.search_docs",
        json!({"query":"oauth setup", "engine_version":"0.1.0"}),
        "different user prompt",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized
            .args
            .get("engine_version")
            .and_then(|v| v.as_str()),
        Some("0.1.0")
    );
}

#[test]
fn normalize_tool_args_get_doc_keeps_existing_path() {
    let normalized = normalize_tool_args(
        "mcp.tandem_mcp.get_doc",
        json!({"path":"/start-here/"}),
        "different user prompt",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("/start-here/")
    );
    assert_eq!(normalized.args_source, "provider_json");
    assert_eq!(normalized.args_integrity, "ok");
}

#[test]
fn normalize_tool_args_pack_builder_infers_goal_from_user_prompt() {
    let user_text =
        "Create a pack that checks latest headline news every day at 8 AM and emails me.";
    let normalized = normalize_tool_args("pack_builder", json!({}), user_text, "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("goal").and_then(|v| v.as_str()),
        Some(user_text)
    );
    assert_eq!(
        normalized.args.get("mode").and_then(|v| v.as_str()),
        Some("preview")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_pack_builder_keeps_existing_goal_and_mode() {
    let normalized = normalize_tool_args(
        "pack_builder",
        json!({"mode":"apply","goal":"existing goal","plan_id":"plan-1"}),
        "new goal should not override",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("goal").and_then(|v| v.as_str()),
        Some("existing goal")
    );
    assert_eq!(
        normalized.args.get("mode").and_then(|v| v.as_str()),
        Some("apply")
    );
    assert_eq!(normalized.args_source, "provider_json");
    assert_eq!(normalized.args_integrity, "ok");
}

#[test]
fn normalize_tool_args_pack_builder_confirm_reuses_plan_from_context() {
    let assistant_context =
        "Pack Builder Preview\n- Plan ID: plan-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let normalized = normalize_tool_args("pack_builder", json!({}), "confirm", assistant_context);
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("mode").and_then(|v| v.as_str()),
        Some("apply")
    );
    assert_eq!(
        normalized.args.get("plan_id").and_then(|v| v.as_str()),
        Some("plan-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
    );
    assert_eq!(
        normalized
            .args
            .get("approve_pack_install")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(normalized.args_source, "recovered_from_context");
}

#[test]
fn normalize_tool_args_pack_builder_apply_recovers_missing_plan_id() {
    let assistant_context =
        "{\"mode\":\"preview\",\"plan_id\":\"plan-11111111-2222-3333-4444-555555555555\"}";
    let normalized = normalize_tool_args(
        "pack_builder",
        json!({"mode":"apply"}),
        "yes",
        assistant_context,
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("mode").and_then(|v| v.as_str()),
        Some("apply")
    );
    assert_eq!(
        normalized.args.get("plan_id").and_then(|v| v.as_str()),
        Some("plan-11111111-2222-3333-4444-555555555555")
    );
}

#[test]
fn normalize_tool_args_pack_builder_short_new_goal_does_not_force_apply() {
    let assistant_context =
        "Pack Builder Preview\n- Plan ID: plan-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let normalized = normalize_tool_args(
        "pack_builder",
        json!({}),
        "create jira sync",
        assistant_context,
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("mode").and_then(|v| v.as_str()),
        Some("preview")
    );
    assert_eq!(
        normalized.args.get("goal").and_then(|v| v.as_str()),
        Some("create jira sync")
    );
}

#[test]
fn normalize_tool_args_write_requires_path() {
    let normalized = normalize_tool_args("write", json!({}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn persisted_failed_tool_args_prefers_normalized_when_raw_is_empty() {
    let args = persisted_failed_tool_args(
        &json!({}),
        &json!({"path":"game.html","content":"<html></html>"}),
    );
    assert_eq!(args["path"], "game.html");
    assert_eq!(args["content"], "<html></html>");
}

#[test]
fn persisted_failed_tool_args_keeps_non_empty_raw_payload() {
    let args = persisted_failed_tool_args(
        &json!("path=game.html content"),
        &json!({"path":"game.html"}),
    );
    assert_eq!(args, json!("path=game.html content"));
}

#[test]
fn normalize_tool_args_write_recovers_alias_path_key() {
    let normalized = normalize_tool_args(
        "write",
        json!({"filePath":"docs/CONCEPT.md","content":"hello"}),
        "",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("docs/CONCEPT.md")
    );
    assert_eq!(
        normalized.args.get("content").and_then(|v| v.as_str()),
        Some("hello")
    );
}

#[test]
fn normalize_tool_args_write_recovers_html_output_target_path() {
    let normalized = normalize_tool_args_with_mode(
        "write",
        json!({"content":"<html></html>"}),
        "Execute task.\n\nRequired output target:\n{\n  \"path\": \"game.html\",\n  \"kind\": \"source\",\n  \"operation\": \"create_or_update\"\n}\n",
        "",
        WritePathRecoveryMode::OutputTargetOnly,
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("game.html")
    );
}

#[test]
fn normalize_tool_args_read_infers_path_from_user_prompt() {
    let normalized = normalize_tool_args(
        "read",
        json!({}),
        "Please inspect `FEATURE_LIST.md` and summarize key sections.",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("FEATURE_LIST.md")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_read_does_not_infer_path_from_assistant_context() {
    let normalized = normalize_tool_args(
        "read",
        json!({}),
        "generic instruction",
        "I will read src-tauri/src/orchestrator/engine.rs first.",
    );
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_write_recovers_path_from_nested_array_payload() {
    let normalized = normalize_tool_args(
        "write",
        json!({"args":[{"file_path":"docs/CONCEPT.md"}],"content":"hello"}),
        "",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("docs/CONCEPT.md")
    );
}

#[test]
fn normalize_tool_args_write_recovers_content_alias() {
    let normalized = normalize_tool_args(
        "write",
        json!({"path":"docs/FEATURES.md","body":"feature notes"}),
        "",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("content").and_then(|v| v.as_str()),
        Some("feature notes")
    );
}

#[test]
fn normalize_tool_args_write_fails_when_content_missing() {
    let normalized = normalize_tool_args("write", json!({"path":"docs/FEATURES.md"}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("WRITE_CONTENT_MISSING")
    );
}

#[test]
fn normalize_tool_args_write_output_target_only_rejects_freeform_guess() {
    let normalized = normalize_tool_args_with_mode(
        "write",
        json!({}),
        "Please implement the screen/state structure in the workspace.",
        "",
        WritePathRecoveryMode::OutputTargetOnly,
    );
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_write_output_target_only_recovers_from_dot_slash_path() {
    let normalized = normalize_tool_args_with_mode(
        "write",
        json!({"path":"./","content":"{}"}),
        "Required Workspace Output:\n- Create or update `.tandem/runs/automation-v2-run-123/artifacts/research-sources.json` relative to the workspace root.",
        "",
        WritePathRecoveryMode::OutputTargetOnly,
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some(".tandem/runs/automation-v2-run-123/artifacts/research-sources.json")
    );
}

#[test]
fn normalize_tool_args_write_recovers_content_from_assistant_context() {
    let normalized = normalize_tool_args(
        "write",
        json!({"path":"docs/FEATURES.md"}),
        "",
        "## Features\n\n- Neon arcade gameplay\n- Single-file HTML structure\n",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("docs/FEATURES.md")
    );
    assert_eq!(
        normalized.args.get("content").and_then(|v| v.as_str()),
        Some("## Features\n\n- Neon arcade gameplay\n- Single-file HTML structure")
    );
    assert_eq!(normalized.args_source, "recovered_from_context");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_write_recovers_raw_nested_string_content() {
    let normalized = normalize_tool_args(
        "write",
        json!({"path":"docs/FEATURES.md","args":"Line 1\nLine 2"}),
        "",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("docs/FEATURES.md")
    );
    assert_eq!(
        normalized.args.get("content").and_then(|v| v.as_str()),
        Some("Line 1\nLine 2")
    );
}

#[test]
fn normalize_tool_args_write_does_not_treat_path_as_content() {
    let normalized = normalize_tool_args("write", json!("docs/FEATURES.md"), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("WRITE_CONTENT_MISSING")
    );
}

#[test]
fn normalize_tool_args_gmail_send_email_omits_empty_attachment() {
    let normalized = normalize_tool_args(
        "gmail_send_email",
        json!({
            "to": "user123@example.com",
            "subject": "Test",
            "body": "Hello",
            "attachment": {
                "s3key": ""
            }
        }),
        "",
        "",
    );
    assert!(normalized.args.get("attachment").is_none());
    assert_eq!(normalized.args_source, "sanitized_attachment");
}

#[test]
fn normalize_tool_args_gmail_send_email_keeps_valid_attachment() {
    let normalized = normalize_tool_args(
        "gmail_send_email",
        json!({
            "to": "user123@example.com",
            "subject": "Test",
            "body": "Hello",
            "attachment": {
                "s3key": "file_123"
            }
        }),
        "",
        "",
    );
    assert_eq!(
        normalized
            .args
            .get("attachment")
            .and_then(|value| value.get("s3key"))
            .and_then(|value| value.as_str()),
        Some("file_123")
    );
}

#[test]
fn classify_required_tool_failure_detects_empty_provider_write_args() {
    let reason = classify_required_tool_failure(
        &[String::from("WRITE_ARGS_EMPTY_FROM_PROVIDER")],
        true,
        1,
        false,
        false,
    );
    assert_eq!(reason, RequiredToolFailureKind::WriteArgsEmptyFromProvider);
}

#[test]
fn normalize_tool_args_read_infers_path_from_bold_markdown() {
    let normalized = normalize_tool_args(
        "read",
        json!({}),
        "Please read **FEATURE_LIST.md** and summarize.",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("FEATURE_LIST.md")
    );
}

#[test]
fn normalize_tool_args_shell_infers_command_from_user_prompt() {
    let normalized = normalize_tool_args("bash", json!({}), "Run `rg -n \"TODO\" .`", "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("command").and_then(|v| v.as_str()),
        Some("rg -n \"TODO\" .")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_read_rejects_root_only_path() {
    let normalized = normalize_tool_args("read", json!({"path":"/"}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_read_recovers_when_provider_path_is_root_only() {
    let normalized =
        normalize_tool_args("read", json!({"path":"/"}), "Please open `CONCEPT.md`", "");
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("CONCEPT.md")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_args_read_rejects_tool_call_markup_path() {
    let normalized = normalize_tool_args(
        "read",
        json!({
            "path":"<tool_call>\n<function=glob>\n<parameter=pattern>**/*</parameter>\n</function>\n</tool_call>"
        }),
        "",
        "",
    );
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_read_rejects_glob_pattern_path() {
    let normalized = normalize_tool_args("read", json!({"path":"**/*"}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_read_rejects_placeholder_path() {
    let normalized = normalize_tool_args("read", json!({"path":"files/directories"}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_read_rejects_tool_policy_placeholder_path() {
    let normalized = normalize_tool_args("read", json!({"path":"tool/policy"}), "", "");
    assert!(normalized.missing_terminal);
    assert_eq!(
        normalized.missing_terminal_reason.as_deref(),
        Some("FILE_PATH_MISSING")
    );
}

#[test]
fn normalize_tool_args_read_recovers_pdf_path_from_user_text() {
    let normalized = normalize_tool_args(
        "read",
        json!({"path":"tool/policy"}),
        "Read `T1011U kitöltési útmutató.pdf` and summarize.",
        "",
    );
    assert!(!normalized.missing_terminal);
    assert_eq!(
        normalized.args.get("path").and_then(|v| v.as_str()),
        Some("T1011U kitöltési útmutató.pdf")
    );
    assert_eq!(normalized.args_source, "inferred_from_user");
    assert_eq!(normalized.args_integrity, "recovered");
}

#[test]
fn normalize_tool_name_strips_default_api_namespace() {
    assert_eq!(normalize_tool_name("default_api:read"), "read");
    assert_eq!(normalize_tool_name("functions.shell"), "bash");
}

#[test]
fn mcp_server_from_tool_name_parses_server_segment() {
    assert_eq!(
        mcp_server_from_tool_name("mcp.arcade.jira_getboards"),
        Some("arcade")
    );
    assert_eq!(mcp_server_from_tool_name("read"), None);
    assert_eq!(mcp_server_from_tool_name("mcp"), None);
}

#[test]
fn mcp_tools_are_exempt_from_workspace_sandbox_path_checks() {
    assert!(is_mcp_tool_name("mcp_list"));
    assert!(is_mcp_tool_name("mcp.tandem_mcp.get_doc"));
    assert!(is_mcp_tool_name("MCP.TANDEM_MCP.GET_DOC"));
    assert!(!is_mcp_tool_name("read"));
    assert!(!is_mcp_tool_name("glob"));
    assert!(is_mcp_sandbox_exempt_server("tandem_mcp"));
    assert!(is_mcp_sandbox_exempt_server("tandem-mcp"));
}

#[test]
fn batch_helpers_use_name_when_tool_is_wrapper() {
    let args = json!({
        "tool_calls":[
            {"tool":"default_api","name":"read","args":{"path":"CONCEPT.md"}},
            {"tool":"default_api:glob","args":{"pattern":"*.md"}}
        ]
    });
    let calls = extract_batch_calls(&args);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, "read");
    assert_eq!(calls[1].0, "glob");
    assert!(is_read_only_batch_call(&args));
    let sig = batch_tool_signature(&args).unwrap_or_default();
    assert!(sig.contains("read:"));
    assert!(sig.contains("glob:"));
}

#[test]
fn batch_helpers_resolve_nested_function_name() {
    let args = json!({
        "tool_calls":[
            {"tool":"default_api","function":{"name":"read"},"args":{"path":"CONCEPT.md"}}
        ]
    });
    let calls = extract_batch_calls(&args);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "read");
    assert!(is_read_only_batch_call(&args));
}

#[test]
fn batch_output_classifier_detects_non_productive_unknown_results() {
    let output = r#"
[
  {"tool":"default_api","output":"Unknown tool: default_api","metadata":{}},
  {"tool":"default_api","output":"Unknown tool: default_api","metadata":{}}
]
"#;
    assert!(is_non_productive_batch_output(output));
}

#[test]
fn runtime_prompt_includes_execution_environment_block() {
    let prompt = tandem_runtime_system_prompt(
        &HostRuntimeContext {
            os: HostOs::Windows,
            arch: "x86_64".to_string(),
            shell_family: ShellFamily::Powershell,
            path_style: PathStyle::Windows,
        },
        &[],
    );
    assert!(prompt.contains("[Execution Environment]"));
    assert!(prompt.contains("Host OS: windows"));
    assert!(prompt.contains("Shell: powershell"));
    assert!(prompt.contains("Path style: windows"));
}

#[test]
fn runtime_prompt_includes_connected_integrations_block() {
    let prompt = tandem_runtime_system_prompt(
        &HostRuntimeContext {
            os: HostOs::Linux,
            arch: "x86_64".to_string(),
            shell_family: ShellFamily::Posix,
            path_style: PathStyle::Posix,
        },
        &["notion".to_string(), "github".to_string()],
    );
    assert!(prompt.contains("[Connected Integrations]"));
    assert!(prompt.contains("- notion"));
    assert!(prompt.contains("- github"));
}

#[test]
fn detects_web_research_prompt_keywords() {
    assert!(requires_web_research_prompt(
        "research todays top news stories and include links"
    ));
    assert!(requires_web_research_prompt(
        "Use web_research and web_fetch to collect current market coverage"
    ));
    assert!(!requires_web_research_prompt(
        "Synthesize the upstream web research artifact into the final report body; do not repeat discovery or fresh web research"
    ));
    assert!(!requires_web_research_prompt(
        "say hello and summarize this text"
    ));
}

#[test]
fn detects_email_delivery_prompt_keywords() {
    assert!(requires_email_delivery_prompt(
        "send a full report with links to user123@example.com"
    ));
    assert!(!requires_email_delivery_prompt("draft a summary for later"));
}

#[test]
fn completion_claim_detector_flags_sent_language() {
    assert!(completion_claims_email_sent(
        "Email Status: Sent to user123@example.com."
    ));
    assert!(!completion_claims_email_sent(
        "I could not send email in this run."
    ));
}
