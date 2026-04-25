use futures::future::BoxFuture;
use serde_json::{Map, Value};
use tandem_providers::ChatMessage;
use tandem_types::{EngineEvent, ToolProgressEvent, ToolProgressSink};

use crate::EventBus;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnowledgebaseGroundingPolicy {
    pub required: bool,
    pub strict: bool,
    pub server_names: Vec<String>,
    pub tool_patterns: Vec<String>,
}

#[derive(Default)]
pub(super) struct StreamedToolCall {
    pub(super) name: String,
    pub(super) args: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RawToolArgsState {
    Present,
    Empty,
    Unparseable,
}

impl RawToolArgsState {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Empty => "empty",
            Self::Unparseable => "unparseable",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WritePathRecoveryMode {
    Heuristic,
    OutputTargetOnly,
}

#[derive(Debug, Clone)]
pub struct SpawnAgentToolContext {
    pub session_id: String,
    pub message_id: String,
    pub tool_call_id: Option<String>,
    pub args: Value,
}

#[derive(Debug, Clone)]
pub struct SpawnAgentToolResult {
    pub output: String,
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct ToolPolicyContext {
    pub session_id: String,
    pub message_id: String,
    pub tool: String,
    pub args: Value,
}

#[derive(Debug, Clone)]
pub struct ToolPolicyDecision {
    pub allowed: bool,
    pub reason: Option<String>,
}

#[derive(Clone)]
pub(super) struct EngineToolProgressSink {
    pub(super) event_bus: EventBus,
    pub(super) session_id: String,
    pub(super) message_id: String,
    pub(super) tool_call_id: Option<String>,
    pub(super) source_tool: String,
}

impl ToolProgressSink for EngineToolProgressSink {
    fn publish(&self, event: ToolProgressEvent) {
        let properties = merge_tool_progress_properties(
            event.properties,
            &self.session_id,
            &self.message_id,
            self.tool_call_id.as_deref(),
            &self.source_tool,
        );
        self.event_bus
            .publish(EngineEvent::new(event.event_type, properties));
    }
}

pub(super) fn merge_tool_progress_properties(
    properties: Value,
    session_id: &str,
    message_id: &str,
    tool_call_id: Option<&str>,
    source_tool: &str,
) -> Value {
    let mut base = Map::new();
    base.insert(
        "sessionID".to_string(),
        Value::String(session_id.to_string()),
    );
    base.insert(
        "messageID".to_string(),
        Value::String(message_id.to_string()),
    );
    base.insert(
        "sourceTool".to_string(),
        Value::String(source_tool.to_string()),
    );
    if let Some(tool_call_id) = tool_call_id {
        base.insert(
            "toolCallID".to_string(),
            Value::String(tool_call_id.to_string()),
        );
    }
    match properties {
        Value::Object(mut map) => {
            for (key, value) in base {
                map.insert(key, value);
            }
            Value::Object(map)
        }
        other => {
            base.insert("data".to_string(), other);
            Value::Object(base)
        }
    }
}

pub trait SpawnAgentHook: Send + Sync {
    fn spawn_agent(
        &self,
        ctx: SpawnAgentToolContext,
    ) -> BoxFuture<'static, anyhow::Result<SpawnAgentToolResult>>;
}

pub trait ToolPolicyHook: Send + Sync {
    fn evaluate_tool(
        &self,
        ctx: ToolPolicyContext,
    ) -> BoxFuture<'static, anyhow::Result<ToolPolicyDecision>>;
}

#[derive(Debug, Clone)]
pub struct PromptContextHookContext {
    pub session_id: String,
    pub message_id: String,
    pub provider_id: String,
    pub model_id: String,
    pub iteration: usize,
}

pub trait PromptContextHook: Send + Sync {
    fn augment_provider_messages(
        &self,
        ctx: PromptContextHookContext,
        messages: Vec<ChatMessage>,
    ) -> BoxFuture<'static, anyhow::Result<Vec<ChatMessage>>>;
}
