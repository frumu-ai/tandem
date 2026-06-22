use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::WorkflowValidationSeverity;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowActionValidationMode {
    #[default]
    Local,
    Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowActionKind {
    ApprovalGate,
    EventEmit,
    ResourcePut,
    ResourcePatch,
    ResourceDelete,
    Tool,
    Capability,
    Workflow,
    Agent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowActionDefinition {
    pub kind: WorkflowActionKind,
    pub name: String,
    #[serde(default)]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowResolvedAction {
    pub kind: WorkflowActionKind,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition: Option<WorkflowActionDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowActionValidationIssue {
    pub severity: WorkflowValidationSeverity,
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowActionRegistry {
    #[serde(default)]
    capabilities: BTreeMap<String, WorkflowActionDefinition>,
    #[serde(default)]
    tools: BTreeMap<String, WorkflowActionDefinition>,
}

impl Default for WorkflowActionRegistry {
    fn default() -> Self {
        let mut registry = Self {
            capabilities: BTreeMap::new(),
            tools: BTreeMap::new(),
        };
        for capability in [
            "planner",
            "verifier.run",
            "classifier.run",
            "kanban.update",
            "slack.notify",
        ] {
            registry.register_capability_schema(capability, optional_object_schema());
        }
        registry
    }
}

impl WorkflowActionRegistry {
    pub fn new() -> Self {
        Self {
            capabilities: BTreeMap::new(),
            tools: BTreeMap::new(),
        }
    }

    pub fn with_default_actions() -> Self {
        Self::default()
    }

    pub fn register_capability_schema(&mut self, capability_id: impl Into<String>, schema: Value) {
        let name = capability_id.into();
        let key = canonical_action_key(&name);
        self.capabilities.insert(
            key,
            WorkflowActionDefinition {
                kind: WorkflowActionKind::Capability,
                name,
                input_schema: schema,
            },
        );
    }

    pub fn with_capability_schema(
        mut self,
        capability_id: impl Into<String>,
        schema: Value,
    ) -> Self {
        self.register_capability_schema(capability_id, schema);
        self
    }

    pub fn register_tool_schema(&mut self, tool_name: impl Into<String>, schema: Value) {
        let name = tool_name.into();
        let key = canonical_action_key(&name);
        self.tools.insert(
            key,
            WorkflowActionDefinition {
                kind: WorkflowActionKind::Tool,
                name,
                input_schema: schema,
            },
        );
    }

    pub fn with_tool_schema(mut self, tool_name: impl Into<String>, schema: Value) -> Self {
        self.register_tool_schema(tool_name, schema);
        self
    }

    pub fn register_tool(&mut self, schema: &tandem_types::ToolSchema) {
        self.register_tool_schema(schema.name.clone(), schema.input_schema.clone());
    }

    pub fn resolve_action(&self, action: &str) -> Result<WorkflowResolvedAction, String> {
        let trimmed = action.trim();
        if trimmed.is_empty() {
            return Err("action is empty".to_string());
        }
        if trimmed == "approval:gate" || trimmed == "approval.gate" {
            return Ok(WorkflowResolvedAction {
                kind: WorkflowActionKind::ApprovalGate,
                action: "approval:gate".to_string(),
                target: None,
                definition: Some(WorkflowActionDefinition {
                    kind: WorkflowActionKind::ApprovalGate,
                    name: "approval:gate".to_string(),
                    input_schema: approval_gate_schema(),
                }),
            });
        }
        if let Some(target) = trimmed.strip_prefix("event:") {
            return self.resolve_prefixed(
                WorkflowActionKind::EventEmit,
                trimmed,
                target,
                optional_object_schema(),
                "event type",
            );
        }
        if let Some(target) = trimmed.strip_prefix("resource:put:") {
            return self.resolve_prefixed(
                WorkflowActionKind::ResourcePut,
                trimmed,
                target,
                optional_object_schema(),
                "resource key",
            );
        }
        if let Some(target) = trimmed.strip_prefix("resource:patch:") {
            return self.resolve_prefixed(
                WorkflowActionKind::ResourcePatch,
                trimmed,
                target,
                optional_object_schema(),
                "resource key",
            );
        }
        if let Some(target) = trimmed.strip_prefix("resource:delete:") {
            return self.resolve_prefixed(
                WorkflowActionKind::ResourceDelete,
                trimmed,
                target,
                optional_object_schema(),
                "resource key",
            );
        }
        if let Some(target) = trimmed.strip_prefix("agent:") {
            return self.resolve_prefixed(
                WorkflowActionKind::Agent,
                trimmed,
                target,
                agent_action_schema(),
                "agent id",
            );
        }
        if let Some(target) = trimmed.strip_prefix("workflow:") {
            return self.resolve_prefixed(
                WorkflowActionKind::Workflow,
                trimmed,
                target,
                optional_object_schema(),
                "workflow id",
            );
        }
        if let Some(target) = trimmed.strip_prefix("tool:") {
            return self.resolve_catalog_action(
                WorkflowActionKind::Tool,
                trimmed,
                target,
                &self.tools,
                "tool",
            );
        }
        if let Some(target) = trimmed.strip_prefix("capability:") {
            return self.resolve_catalog_action(
                WorkflowActionKind::Capability,
                trimmed,
                target,
                &self.capabilities,
                "capability",
            );
        }
        self.resolve_catalog_action(
            WorkflowActionKind::Capability,
            trimmed,
            trimmed,
            &self.capabilities,
            "capability",
        )
    }

    pub fn validate_action(
        &self,
        action: &str,
        with: Option<&Value>,
        mode: WorkflowActionValidationMode,
    ) -> Vec<WorkflowActionValidationIssue> {
        let mut issues = Vec::new();
        match self.resolve_action(action) {
            Ok(resolved) => {
                if let Some(definition) = resolved.definition.as_ref() {
                    issues.extend(validate_action_input(&definition.input_schema, with));
                } else {
                    issues.push(unknown_action_issue(action, &resolved, mode));
                }
                if matches!(resolved.kind, WorkflowActionKind::Workflow) {
                    issues.push(WorkflowActionValidationIssue {
                        severity: match mode {
                            WorkflowActionValidationMode::Strict => WorkflowValidationSeverity::Error,
                            WorkflowActionValidationMode::Local => WorkflowValidationSeverity::Warning,
                        },
                        field: "action".to_string(),
                        message: format!(
                            "workflow action `{}` is recognized but nested workflow execution is not supported yet",
                            action.trim()
                        ),
                    });
                }
            }
            Err(message) => issues.push(WorkflowActionValidationIssue {
                severity: WorkflowValidationSeverity::Error,
                field: "action".to_string(),
                message,
            }),
        }
        issues
    }

    fn resolve_prefixed(
        &self,
        kind: WorkflowActionKind,
        action: &str,
        target: &str,
        input_schema: Value,
        target_label: &str,
    ) -> Result<WorkflowResolvedAction, String> {
        let target = target.trim();
        if target.is_empty() {
            return Err(format!(
                "workflow action `{action}` is missing {target_label}"
            ));
        }
        Ok(WorkflowResolvedAction {
            kind: kind.clone(),
            action: action.to_string(),
            target: Some(target.to_string()),
            definition: Some(WorkflowActionDefinition {
                kind,
                name: action.to_string(),
                input_schema,
            }),
        })
    }

    fn resolve_catalog_action(
        &self,
        kind: WorkflowActionKind,
        action: &str,
        target: &str,
        catalog: &BTreeMap<String, WorkflowActionDefinition>,
        target_label: &str,
    ) -> Result<WorkflowResolvedAction, String> {
        let target = target.trim();
        if target.is_empty() {
            return Err(format!(
                "workflow action `{action}` is missing {target_label} name"
            ));
        }
        let definition = catalog.get(&canonical_action_key(target)).cloned();
        Ok(WorkflowResolvedAction {
            kind,
            action: action.to_string(),
            target: Some(target.to_string()),
            definition,
        })
    }
}

fn validate_action_input(
    schema: &Value,
    with: Option<&Value>,
) -> Vec<WorkflowActionValidationIssue> {
    if schema.is_null() || schema == &json!({}) {
        return Vec::new();
    }
    match with {
        Some(payload) => validate_json_schema(schema, payload, "with"),
        None if schema_has_required_fields(schema) => vec![WorkflowActionValidationIssue {
            severity: WorkflowValidationSeverity::Error,
            field: "with".to_string(),
            message: "`with` is required for this workflow action".to_string(),
        }],
        None => Vec::new(),
    }
}

fn unknown_action_issue(
    action: &str,
    resolved: &WorkflowResolvedAction,
    mode: WorkflowActionValidationMode,
) -> WorkflowActionValidationIssue {
    let target = resolved.target.as_deref().unwrap_or_else(|| action.trim());
    let (field, subject) = match resolved.kind {
        WorkflowActionKind::Tool if target.starts_with("mcp.") => {
            ("action", format!("MCP tool `{target}`"))
        }
        WorkflowActionKind::Tool => ("action", format!("tool `{target}`")),
        WorkflowActionKind::Capability => ("action", format!("capability `{target}`")),
        _ => ("action", format!("action `{}`", action.trim())),
    };
    WorkflowActionValidationIssue {
        severity: match mode {
            WorkflowActionValidationMode::Strict => WorkflowValidationSeverity::Error,
            WorkflowActionValidationMode::Local => WorkflowValidationSeverity::Warning,
        },
        field: field.to_string(),
        message: format!("{subject} is not present in the workflow action catalog"),
    }
}

fn validate_json_schema(
    schema: &Value,
    value: &Value,
    path: &str,
) -> Vec<WorkflowActionValidationIssue> {
    let mut issues = Vec::new();
    if let Some(types) = schema_types(schema) {
        if !types
            .iter()
            .any(|schema_type| value_matches_type(value, schema_type))
        {
            issues.push(WorkflowActionValidationIssue {
                severity: WorkflowValidationSeverity::Error,
                field: path.to_string(),
                message: format!("`{path}` must be {}", types.join(" or ")),
            });
            return issues;
        }
    }
    if let Some(values) = schema.get("enum").and_then(Value::as_array) {
        if !values.iter().any(|candidate| candidate == value) {
            issues.push(WorkflowActionValidationIssue {
                severity: WorkflowValidationSeverity::Error,
                field: path.to_string(),
                message: format!("`{path}` must match one of the declared enum values"),
            });
        }
    }
    if let Some(object) = value.as_object() {
        if let Some(required) = schema.get("required").and_then(Value::as_array) {
            for key in required.iter().filter_map(Value::as_str) {
                if !object.contains_key(key) {
                    issues.push(WorkflowActionValidationIssue {
                        severity: WorkflowValidationSeverity::Error,
                        field: format!("{path}.{key}"),
                        message: format!("`{path}.{key}` is required"),
                    });
                }
            }
        }
        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(properties) = properties {
            for (key, child_schema) in properties {
                if let Some(child) = object.get(key) {
                    issues.extend(validate_json_schema(
                        child_schema,
                        child,
                        &format!("{path}.{key}"),
                    ));
                }
            }
        }
        if schema
            .get("additionalProperties")
            .and_then(Value::as_bool)
            .is_some_and(|allowed| !allowed)
        {
            for key in object.keys() {
                if !properties.is_some_and(|properties| properties.contains_key(key)) {
                    issues.push(WorkflowActionValidationIssue {
                        severity: WorkflowValidationSeverity::Error,
                        field: format!("{path}.{key}"),
                        message: format!("`{path}.{key}` is not allowed by this action schema"),
                    });
                }
            }
        }
    }
    if let (Some(items), Some(values)) = (schema.get("items"), value.as_array()) {
        for (idx, item) in values.iter().enumerate() {
            issues.extend(validate_json_schema(items, item, &format!("{path}[{idx}]")));
        }
    }
    issues
}

fn schema_has_required_fields(schema: &Value) -> bool {
    schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| !required.is_empty())
}

fn schema_types(schema: &Value) -> Option<Vec<String>> {
    match schema.get("type") {
        Some(Value::String(schema_type)) => Some(vec![schema_type.clone()]),
        Some(Value::Array(schema_types)) => {
            let types = schema_types
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            (!types.is_empty()).then_some(types)
        }
        _ => None,
    }
}

fn value_matches_type(value: &Value, schema_type: &str) -> bool {
    match schema_type {
        "array" => value.is_array(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
        "null" => value.is_null(),
        "number" => value.is_number(),
        "object" => value.is_object(),
        "string" => value.is_string(),
        _ => true,
    }
}

fn approval_gate_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "title": { "type": "string" },
            "instructions": { "type": "string" },
            "decisions": {
                "type": "array",
                "items": { "type": "string" }
            },
            "rework_targets": {
                "type": "array",
                "items": { "type": "string" }
            }
        }
    })
}

fn agent_action_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "prompt": { "type": "string" }
        }
    })
}

fn optional_object_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": true
    })
}

fn canonical_action_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}
