use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    #[serde(default)]
    pub properties: Value,
}

impl EngineEvent {
    pub fn new(event_type: impl Into<String>, properties: Value) -> Self {
        Self {
            event_type: event_type.into(),
            properties,
        }
    }
}
