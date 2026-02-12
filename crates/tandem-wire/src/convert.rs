use std::collections::HashMap;

use serde_json::{json, Value};
use tandem_types::{Message, MessagePart, ModelSpec, ProviderInfo, Session};

use crate::{
    WireMessageInfo, WireMessagePart, WireMessageTime, WireModelSpec, WireProviderCatalog,
    WireProviderEntry, WireProviderModel, WireProviderModelLimit, WireSession, WireSessionMessage,
    WireSessionTime,
};
use std::sync::atomic::{AtomicU64, Ordering};

static PART_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_part_id() -> String {
    format!("part-{}", PART_SEQ.fetch_add(1, Ordering::Relaxed))
}

fn to_epoch_seconds(dt: chrono::DateTime<chrono::Utc>) -> u64 {
    dt.timestamp().max(0) as u64
}

impl From<ModelSpec> for WireModelSpec {
    fn from(value: ModelSpec) -> Self {
        Self {
            provider_id: value.provider_id,
            model_id: value.model_id,
        }
    }
}

impl From<Session> for WireSession {
    fn from(value: Session) -> Self {
        let session_id = value.id.clone();
        Self {
            id: value.id,
            slug: value.slug,
            version: value.version,
            project_id: value.project_id,
            directory: Some(value.directory),
            title: value.title,
            time: Some(WireSessionTime {
                created: to_epoch_seconds(value.time.created),
                updated: to_epoch_seconds(value.time.updated),
            }),
            model: value.model.map(Into::into),
            provider: value.provider,
            messages: value
                .messages
                .into_iter()
                .map(|m| WireSessionMessage::from_message(&m, &session_id))
                .collect(),
        }
    }
}

impl WireSessionMessage {
    pub fn from_message(msg: &Message, session_id: &str) -> Self {
        let info = WireMessageInfo {
            id: msg.id.clone(),
            session_id: session_id.to_string(),
            role: format!("{:?}", msg.role).to_lowercase(),
            time: WireMessageTime {
                created: to_epoch_seconds(msg.created_at),
                completed: None,
            },
            summary: None,
            agent: None,
            model: None,
            deleted: None,
            reverted: None,
        };

        let parts = msg.parts.iter().map(message_part_to_value).collect();
        Self { info, parts }
    }
}

fn message_part_to_value(part: &MessagePart) -> Value {
    match part {
        MessagePart::Text { text } => json!({"type":"text","text":text}),
        MessagePart::Reasoning { text } => json!({"type":"reasoning","text":text}),
        MessagePart::ToolInvocation {
            tool,
            args,
            result,
            error,
        } => json!({
            "type":"tool",
            "tool": tool,
            "args": args,
            "result": result,
            "error": error
        }),
    }
}

impl WireProviderCatalog {
    pub fn from_providers(providers: Vec<ProviderInfo>, connected: Vec<String>) -> Self {
        let all = providers
            .into_iter()
            .map(|provider| {
                let models = provider
                    .models
                    .into_iter()
                    .map(|model| {
                        (
                            model.id,
                            WireProviderModel {
                                name: Some(model.display_name),
                                limit: Some(WireProviderModelLimit {
                                    context: Some(model.context_window as u32),
                                }),
                            },
                        )
                    })
                    .collect::<HashMap<_, _>>();

                WireProviderEntry {
                    id: provider.id,
                    name: Some(provider.name),
                    models,
                }
            })
            .collect();

        Self { all, connected }
    }
}

impl WireMessagePart {
    pub fn text(session_id: &str, message_id: &str, text: impl Into<String>) -> Self {
        Self {
            id: Some(next_part_id()),
            session_id: Some(session_id.to_string()),
            message_id: Some(message_id.to_string()),
            part_type: Some("text".to_string()),
            text: Some(text.into()),
            tool: None,
            args: None,
            state: None,
            result: None,
            error: None,
        }
    }

    pub fn tool_invocation(
        session_id: &str,
        message_id: &str,
        tool: impl Into<String>,
        args: Value,
    ) -> Self {
        Self {
            id: Some(next_part_id()),
            session_id: Some(session_id.to_string()),
            message_id: Some(message_id.to_string()),
            part_type: Some("tool".to_string()),
            text: None,
            tool: Some(tool.into()),
            args: Some(args),
            state: Some("running".to_string()),
            result: None,
            error: None,
        }
    }

    pub fn tool_result(
        session_id: &str,
        message_id: &str,
        tool: impl Into<String>,
        result: Value,
    ) -> Self {
        Self {
            id: Some(next_part_id()),
            session_id: Some(session_id.to_string()),
            message_id: Some(message_id.to_string()),
            part_type: Some("tool".to_string()),
            text: None,
            tool: Some(tool.into()),
            args: None,
            state: Some("completed".to_string()),
            result: Some(result),
            error: None,
        }
    }
}
