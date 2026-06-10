use futures::future::BoxFuture;
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use tandem_providers::ChatMessage;
use tandem_types::{
    EngineEvent, TenantContext, ToolProgressEvent, ToolProgressSink, VerifiedTenantContext,
};

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
    pub tenant_context: Option<TenantContext>,
    pub verified_tenant_context: Option<VerifiedTenantContext>,
    pub tool: String,
    pub args: Value,
}

#[derive(Debug, Clone)]
pub struct ToolPolicyDecision {
    pub allowed: bool,
    pub reason: Option<String>,
    pub policy_decision_id: Option<String>,
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

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptContextHookSourceStats {
    pub injected_count: usize,
    pub injected_chars: usize,
    pub dropped_count: usize,
    pub dropped_chars: usize,
    pub deferred_count: usize,
    pub deferred_chars: usize,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromptContextHookStats {
    pub budget_chars: Option<usize>,
    pub used_chars: usize,
    pub remaining_chars: Option<usize>,
    pub sources: BTreeMap<String, PromptContextHookSourceStats>,
}

impl PromptContextHookStats {
    pub fn record_injected(&mut self, source: impl Into<String>, count: usize, chars: usize) {
        let entry = self.sources.entry(source.into()).or_default();
        entry.injected_count = entry.injected_count.saturating_add(count);
        entry.injected_chars = entry.injected_chars.saturating_add(chars);
        self.used_chars = self.used_chars.saturating_add(chars);
        self.refresh_remaining();
    }

    pub fn record_dropped(&mut self, source: impl Into<String>, count: usize, chars: usize) {
        let entry = self.sources.entry(source.into()).or_default();
        entry.dropped_count = entry.dropped_count.saturating_add(count);
        entry.dropped_chars = entry.dropped_chars.saturating_add(chars);
    }

    pub fn record_deferred(&mut self, source: impl Into<String>, count: usize, chars: usize) {
        let entry = self.sources.entry(source.into()).or_default();
        entry.deferred_count = entry.deferred_count.saturating_add(count);
        entry.deferred_chars = entry.deferred_chars.saturating_add(chars);
    }

    pub fn injected_count(&self) -> usize {
        self.sources
            .values()
            .map(|source| source.injected_count)
            .sum()
    }

    pub fn injected_chars(&self) -> usize {
        self.sources
            .values()
            .map(|source| source.injected_chars)
            .sum()
    }

    pub fn dropped_count(&self) -> usize {
        self.sources
            .values()
            .map(|source| source.dropped_count)
            .sum()
    }

    pub fn dropped_chars(&self) -> usize {
        self.sources
            .values()
            .map(|source| source.dropped_chars)
            .sum()
    }

    pub fn deferred_count(&self) -> usize {
        self.sources
            .values()
            .map(|source| source.deferred_count)
            .sum()
    }

    pub fn deferred_chars(&self) -> usize {
        self.sources
            .values()
            .map(|source| source.deferred_chars)
            .sum()
    }

    fn refresh_remaining(&mut self) {
        self.remaining_chars = self
            .budget_chars
            .map(|budget| budget.saturating_sub(self.used_chars));
    }
}

#[derive(Debug, Clone)]
pub struct PromptContextHookResult {
    pub messages: Vec<ChatMessage>,
    pub stats: PromptContextHookStats,
}

impl PromptContextHookResult {
    pub fn new(messages: Vec<ChatMessage>, stats: PromptContextHookStats) -> Self {
        Self { messages, stats }
    }
}

pub trait PromptContextHook: Send + Sync {
    fn augment_provider_messages(
        &self,
        ctx: PromptContextHookContext,
        messages: Vec<ChatMessage>,
    ) -> BoxFuture<'static, anyhow::Result<PromptContextHookResult>>;
}
