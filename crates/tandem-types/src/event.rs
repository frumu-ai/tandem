use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime_event::RuntimeEventEnvelope;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub properties: Value,
    /// Canonical envelope (TAN-199), stamped by the event bus at publish
    /// time. Optional so pre-envelope payloads keep deserializing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub envelope: Option<RuntimeEventEnvelope>,
}

impl EngineEvent {
    pub fn new(event_type: impl Into<String>, properties: Value) -> Self {
        Self {
            event_type: event_type.into(),
            properties,
            envelope: None,
        }
    }

    pub fn with_envelope(mut self, envelope: RuntimeEventEnvelope) -> Self {
        self.envelope = Some(envelope);
        self
    }
}
