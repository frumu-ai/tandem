use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolEffect {
    Read,
    Write,
    Delete,
    Search,
    Execute,
    Fetch,
    Patch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolDomain {
    Workspace,
    Web,
    Shell,
    Browser,
    Planning,
    Memory,
    Collaboration,
    Integration,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ToolCapabilities {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<ToolEffect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub domains: Vec<ToolDomain>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub reads_workspace: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub writes_workspace: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub network_access: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub destructive: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub requires_verification: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub preferred_for_discovery: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub preferred_for_validation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "ToolCapabilities::is_empty")]
    pub capabilities: ToolCapabilities,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl ToolCapabilities {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn effect(mut self, effect: ToolEffect) -> Self {
        if !self.effects.contains(&effect) {
            self.effects.push(effect);
        }
        self
    }

    pub fn domain(mut self, domain: ToolDomain) -> Self {
        if !self.domains.contains(&domain) {
            self.domains.push(domain);
        }
        self
    }

    pub fn reads_workspace(mut self) -> Self {
        self.reads_workspace = true;
        self
    }

    pub fn writes_workspace(mut self) -> Self {
        self.writes_workspace = true;
        self
    }

    pub fn network_access(mut self) -> Self {
        self.network_access = true;
        self
    }

    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    pub fn requires_verification(mut self) -> Self {
        self.requires_verification = true;
        self
    }

    pub fn preferred_for_discovery(mut self) -> Self {
        self.preferred_for_discovery = true;
        self
    }

    pub fn preferred_for_validation(mut self) -> Self {
        self.preferred_for_validation = true;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
            && self.domains.is_empty()
            && !self.reads_workspace
            && !self.writes_workspace
            && !self.network_access
            && !self.destructive
            && !self.requires_verification
            && !self.preferred_for_discovery
            && !self.preferred_for_validation
    }
}

impl ToolSchema {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
            capabilities: ToolCapabilities::default(),
        }
    }

    pub fn with_capabilities(mut self, capabilities: ToolCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub output: String,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone)]
pub struct ToolProgressEvent {
    pub event_type: String,
    pub properties: Value,
}

impl ToolProgressEvent {
    pub fn new(event_type: impl Into<String>, properties: Value) -> Self {
        Self {
            event_type: event_type.into(),
            properties,
        }
    }
}

pub trait ToolProgressSink: Send + Sync {
    fn publish(&self, event: ToolProgressEvent);
}

pub type SharedToolProgressSink = Arc<dyn ToolProgressSink>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_schema_deserializes_legacy_payload_without_capabilities() {
        let actual: ToolSchema = serde_json::from_value(serde_json::json!({
            "name": "read",
            "description": "Read file contents",
            "input_schema": {
                "type": "object"
            }
        }))
        .unwrap();

        let expected = ToolSchema::new(
            "read",
            "Read file contents",
            serde_json::json!({
                "type": "object"
            }),
        );

        assert_eq!(actual, expected);
    }

    #[test]
    fn tool_schema_serialization_omits_empty_capabilities() {
        let actual = serde_json::to_value(ToolSchema::new(
            "read",
            "Read file contents",
            serde_json::json!({
                "type": "object"
            }),
        ))
        .unwrap();

        let expected = serde_json::json!({
            "name": "read",
            "description": "Read file contents",
            "input_schema": {
                "type": "object"
            }
        });

        assert_eq!(actual, expected);
    }

    #[test]
    fn tool_schema_round_trips_capabilities() {
        let actual: ToolSchema = serde_json::from_value(serde_json::json!({
            "name": "write",
            "description": "Write file contents",
            "input_schema": {
                "type": "object"
            },
            "capabilities": {
                "effects": ["write"],
                "domains": ["workspace"],
                "writes_workspace": true,
                "requires_verification": true
            }
        }))
        .unwrap();

        let expected = ToolSchema::new(
            "write",
            "Write file contents",
            serde_json::json!({
                "type": "object"
            }),
        )
        .with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Write)
                .domain(ToolDomain::Workspace)
                .writes_workspace()
                .requires_verification(),
        );

        assert_eq!(actual, expected);
    }
}
