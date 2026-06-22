use super::loop_guards::{parse_budget_override, HARD_TOOL_CALL_CEILING};
use super::*;
use crate::{EventBus, Storage};
use std::sync::{Mutex, OnceLock};
use tandem_types::{
    HostOs, PathStyle, PrewriteCoverageMode, PrewriteRequirements, Session, ShellFamily,
};
use uuid::Uuid;

fn env_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static ENV_TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    ENV_TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env test lock")
}

mod suite_a;
mod suite_b;

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

mod context_evals;
mod suite_c;
mod suite_d;
