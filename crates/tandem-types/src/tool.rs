use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tandem_enterprise_contract::{AccessPermission, DataClass, ResourceKind};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolDefaultVisibility {
    #[default]
    Visible,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRiskTier {
    ReadDiscover,
    InternalWrite,
    ExternalDraft,
    ExternalSend,
    CustomerDataAccess,
    SourceCodeMutation,
    FinancialRecordAccess,
    CredentialAdmin,
    DestructiveDelete,
    MoneyMovementContract,
}

impl ToolRiskTier {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadDiscover => "read_discover",
            Self::InternalWrite => "internal_write",
            Self::ExternalDraft => "external_draft",
            Self::ExternalSend => "external_send",
            Self::CustomerDataAccess => "customer_data_access",
            Self::SourceCodeMutation => "source_code_mutation",
            Self::FinancialRecordAccess => "financial_record_access",
            Self::CredentialAdmin => "credential_admin",
            Self::DestructiveDelete => "destructive_delete",
            Self::MoneyMovementContract => "money_movement_contract",
        }
    }

    pub fn approval_required_by_default(self) -> bool {
        matches!(
            self,
            Self::ExternalSend
                | Self::FinancialRecordAccess
                | Self::CredentialAdmin
                | Self::DestructiveDelete
                | Self::MoneyMovementContract
        )
    }

    pub fn hidden_without_grant_by_default(self) -> bool {
        matches!(self, Self::CredentialAdmin)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSecurityDescriptor {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_permissions: Vec<AccessPermission>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_kinds: Vec<ResourceKind>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_classes: Vec<DataClass>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub admin_surface: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub external_side_effect: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub credential_access: bool,
    #[serde(default, skip_serializing_if = "is_default_visibility_visible")]
    pub default_visibility: ToolDefaultVisibility,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_tier: Option<ToolRiskTier>,
}

impl Default for ToolSecurityDescriptor {
    fn default() -> Self {
        Self {
            required_permissions: Vec::new(),
            resource_kinds: Vec::new(),
            data_classes: Vec::new(),
            admin_surface: false,
            external_side_effect: false,
            credential_access: false,
            default_visibility: ToolDefaultVisibility::Visible,
            risk_tier: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    #[serde(default, skip_serializing_if = "ToolCapabilities::is_empty")]
    pub capabilities: ToolCapabilities,
    #[serde(default, skip_serializing_if = "ToolSecurityDescriptor::is_empty")]
    pub security: ToolSecurityDescriptor,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_default_visibility_visible(value: &ToolDefaultVisibility) -> bool {
    matches!(value, ToolDefaultVisibility::Visible)
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

impl ToolSecurityDescriptor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn permission(mut self, permission: AccessPermission) -> Self {
        if !self.required_permissions.contains(&permission) {
            self.required_permissions.push(permission);
        }
        self
    }

    pub fn resource_kind(mut self, resource_kind: ResourceKind) -> Self {
        if !self.resource_kinds.contains(&resource_kind) {
            self.resource_kinds.push(resource_kind);
        }
        self
    }

    pub fn data_class(mut self, data_class: DataClass) -> Self {
        if !self.data_classes.contains(&data_class) {
            self.data_classes.push(data_class);
        }
        self
    }

    pub fn admin_surface(mut self) -> Self {
        self.admin_surface = true;
        self
    }

    pub fn external_side_effect(mut self) -> Self {
        self.external_side_effect = true;
        self
    }

    pub fn credential_access(mut self) -> Self {
        self.credential_access = true;
        self
    }

    pub fn hidden_by_default(mut self) -> Self {
        self.default_visibility = ToolDefaultVisibility::Hidden;
        self
    }

    pub fn risk_tier(mut self, risk_tier: ToolRiskTier) -> Self {
        self.risk_tier = Some(risk_tier);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.required_permissions.is_empty()
            && self.resource_kinds.is_empty()
            && self.data_classes.is_empty()
            && !self.admin_surface
            && !self.external_side_effect
            && !self.credential_access
            && matches!(self.default_visibility, ToolDefaultVisibility::Visible)
            && self.risk_tier.is_none()
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
            security: ToolSecurityDescriptor::default(),
        }
    }

    pub fn with_capabilities(mut self, capabilities: ToolCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_security(mut self, security: ToolSecurityDescriptor) -> Self {
        self.security = security;
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
    fn tool_schema_deserializes_legacy_payload_without_security() {
        let actual: ToolSchema = serde_json::from_value(serde_json::json!({
            "name": "read",
            "description": "Read file contents",
            "input_schema": {
                "type": "object"
            }
        }))
        .unwrap();

        assert!(actual.security.is_empty());
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

    #[test]
    fn tool_schema_round_trips_security_descriptor() {
        let actual: ToolSchema = serde_json::from_value(serde_json::json!({
            "name": "connector_admin",
            "description": "Manage connector credentials",
            "input_schema": {
                "type": "object"
            },
            "security": {
                "required_permissions": ["admin", "execute"],
                "resource_kinds": ["connector_instance", "secret_provider_credential"],
                "data_classes": ["credential"],
                "admin_surface": true,
                "external_side_effect": true,
                "credential_access": true,
                "default_visibility": "hidden"
            }
        }))
        .unwrap();

        let expected = ToolSchema::new(
            "connector_admin",
            "Manage connector credentials",
            serde_json::json!({
                "type": "object"
            }),
        )
        .with_security(
            ToolSecurityDescriptor::new()
                .permission(AccessPermission::Admin)
                .permission(AccessPermission::Execute)
                .resource_kind(ResourceKind::ConnectorInstance)
                .resource_kind(ResourceKind::SecretProviderCredential)
                .data_class(DataClass::Credential)
                .admin_surface()
                .external_side_effect()
                .credential_access()
                .hidden_by_default(),
        );

        assert_eq!(actual, expected);
    }
}
