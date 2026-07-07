use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tandem_enterprise_contract::{
    DataClass, EffectivePolicySnapshot, EffectivePolicySource, EnterprisePolicyEffect,
    EnterprisePolicyScopeLevel, ResourceRef, TenantContext, VerifiedTenantContext,
};

pub const EFFECTIVE_POLICY_METADATA_KEY: &str = "effective_policy";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecisionEffect {
    Allow,
    Deny,
    ApprovalRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct GovernanceRequesterContext {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub org_units: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<String>,
}

impl GovernanceRequesterContext {
    pub fn from_verified_context(context: &VerifiedTenantContext) -> Option<Self> {
        if context.org_units.is_empty() && context.roles.is_empty() {
            return None;
        }
        Some(Self {
            org_units: context.org_units.clone(),
            roles: context.roles.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecisionRecord {
    pub decision_id: String,
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester_context: Option<GovernanceRequesterContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_classes: Vec<DataClass>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_tier: Option<String>,
    pub decision: PolicyDecisionEffect,
    pub reason_code: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_event_id: Option<String>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub metadata: Value,
}

impl PolicyDecisionEffect {
    pub fn enterprise_effect(self) -> EnterprisePolicyEffect {
        match self {
            Self::Allow => EnterprisePolicyEffect::Allow,
            Self::Deny => EnterprisePolicyEffect::Deny,
            Self::ApprovalRequired => EnterprisePolicyEffect::ApprovalRequired,
        }
    }

    pub fn from_enterprise_effect(effect: EnterprisePolicyEffect) -> Self {
        match effect {
            EnterprisePolicyEffect::Allow => Self::Allow,
            EnterprisePolicyEffect::Deny => Self::Deny,
            EnterprisePolicyEffect::ApprovalRequired => Self::ApprovalRequired,
        }
    }
}

impl PolicyDecisionRecord {
    pub fn with_effective_policy_defaults(self) -> Self {
        if self.effective_policy_snapshot().is_some() {
            return self;
        }

        let policy_id = self
            .policy_id
            .clone()
            .unwrap_or_else(|| "unspecified_policy".to_string());
        let source = EffectivePolicySource {
            rule_id: format!("{policy_id}:fallback"),
            policy_id: policy_id.clone(),
            version: 1,
            scope_level: self.default_effective_policy_scope_level(),
            effect: self.decision.enterprise_effect(),
            overridable: false,
            reason_code: "fallback_effective_policy_source".to_string(),
            reason: format!("fallback effective policy source for `{policy_id}`"),
            approval_id: None,
        };
        let mut snapshot = EffectivePolicySnapshot::single_source(
            self.tenant_context.clone(),
            self.created_at_ms,
            source,
        );
        snapshot.reason_code = self.reason_code.clone();
        snapshot.reason = self.reason.clone();
        snapshot.approval_id = self.approval_id.clone();
        self.with_effective_policy_snapshot(snapshot)
    }

    pub fn with_effective_policy_snapshot(mut self, snapshot: EffectivePolicySnapshot) -> Self {
        let snapshot_value =
            serde_json::to_value(snapshot).unwrap_or_else(|_| Value::Object(Map::new()));
        let mut metadata = match self.metadata {
            Value::Object(metadata) => metadata,
            Value::Null => Map::new(),
            value => {
                let mut metadata = Map::new();
                metadata.insert("legacy".to_string(), value);
                metadata
            }
        };
        metadata.insert(EFFECTIVE_POLICY_METADATA_KEY.to_string(), snapshot_value);
        self.metadata = Value::Object(metadata);
        self
    }

    pub fn apply_effective_policy_snapshot(mut self, snapshot: EffectivePolicySnapshot) -> Self {
        self.decision = PolicyDecisionEffect::from_enterprise_effect(snapshot.effect);
        self.reason_code = snapshot.reason_code.clone();
        self.reason = snapshot.reason.clone();
        self.policy_id = snapshot
            .decision_source
            .as_ref()
            .map(|source| source.policy_id.clone())
            .or_else(|| Some("enterprise_policy_resolver".to_string()));
        self.approval_id = snapshot.approval_id.clone();
        self.with_effective_policy_snapshot(snapshot)
    }

    fn default_effective_policy_scope_level(&self) -> EnterprisePolicyScopeLevel {
        if self.has_workflow_phase_metadata() {
            EnterprisePolicyScopeLevel::Phase
        } else if self.resource.is_some() {
            EnterprisePolicyScopeLevel::Resource
        } else if self.automation_id.is_some() || self.node_id.is_some() || self.run_id.is_some() {
            EnterprisePolicyScopeLevel::Workflow
        } else {
            EnterprisePolicyScopeLevel::Tenant
        }
    }

    fn has_workflow_phase_metadata(&self) -> bool {
        let phase = self
            .metadata
            .pointer("/phase_tool_authority/phase")
            .or_else(|| self.metadata.pointer("/workflow_phase"))
            .or_else(|| self.metadata.pointer("/workflowPhase"))
            .and_then(Value::as_str);

        phase.map(str::trim).is_some_and(|phase| !phase.is_empty())
    }

    pub fn effective_policy_snapshot(&self) -> Option<EffectivePolicySnapshot> {
        self.metadata
            .get(EFFECTIVE_POLICY_METADATA_KEY)
            .and_then(|value| serde_json::from_value(value.clone()).ok())
    }

    pub fn effective_policy_version_id(&self) -> Option<String> {
        self.effective_policy_snapshot()
            .map(|snapshot| snapshot.policy_version_id)
    }

    pub fn inherited_policy_sources(&self) -> Vec<EffectivePolicySource> {
        self.effective_policy_snapshot()
            .map(|snapshot| snapshot.inherited_sources)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_enterprise_contract::ResourceKind;

    #[test]
    fn policy_decision_defaults_record_effective_policy_metadata() {
        let record = PolicyDecisionRecord {
            decision_id: "decision-1".to_string(),
            tenant_context: TenantContext::explicit_user_workspace(
                "acme",
                "finance",
                None,
                "user-finance",
            ),
            requester_context: None,
            actor_id: Some("user-finance".to_string()),
            session_id: None,
            message_id: None,
            run_id: Some("run-1".to_string()),
            automation_id: None,
            node_id: None,
            tool: Some("mcp.bank.release_funds".to_string()),
            resource: None,
            data_classes: vec![DataClass::FinancialRecord],
            risk_tier: Some("money_movement".to_string()),
            decision: PolicyDecisionEffect::ApprovalRequired,
            reason_code: "approval_required".to_string(),
            reason: "approval required".to_string(),
            policy_id: Some("fintech_strict".to_string()),
            grant_id: None,
            approval_id: Some("approval-1".to_string()),
            audit_event_id: None,
            created_at_ms: 1_000,
            metadata: Value::Null,
        }
        .with_effective_policy_defaults();

        let snapshot = record
            .effective_policy_snapshot()
            .expect("effective policy snapshot metadata");
        assert_eq!(snapshot.effect, EnterprisePolicyEffect::ApprovalRequired);
        assert_eq!(snapshot.reason_code, "approval_required");
        assert_eq!(snapshot.reason, "approval required");
        assert_eq!(snapshot.approval_id.as_deref(), Some("approval-1"));
        assert_eq!(
            record.effective_policy_version_id().as_deref(),
            Some(snapshot.policy_version_id.as_str())
        );
        assert_eq!(record.inherited_policy_sources().len(), 1);
        assert_eq!(
            record.inherited_policy_sources()[0].scope_level,
            EnterprisePolicyScopeLevel::Workflow
        );
        assert_eq!(
            record.inherited_policy_sources()[0].reason_code,
            "fallback_effective_policy_source"
        );
    }

    #[test]
    fn policy_decision_defaults_infer_resource_scope_without_phase_metadata() {
        let record = PolicyDecisionRecord {
            decision_id: "decision-resource".to_string(),
            tenant_context: TenantContext::explicit_user_workspace(
                "acme",
                "finance",
                None,
                "user-finance",
            ),
            requester_context: None,
            actor_id: Some("user-finance".to_string()),
            session_id: None,
            message_id: None,
            run_id: None,
            automation_id: None,
            node_id: None,
            tool: None,
            resource: Some(ResourceRef::new(
                "acme",
                "finance",
                ResourceKind::DataRoom,
                "ledger",
            )),
            data_classes: vec![DataClass::FinancialRecord],
            risk_tier: None,
            decision: PolicyDecisionEffect::Deny,
            reason_code: "authority_denied".to_string(),
            reason: "resource access denied".to_string(),
            policy_id: Some("intra_tenant_authority".to_string()),
            grant_id: None,
            approval_id: None,
            audit_event_id: None,
            created_at_ms: 1_000,
            metadata: Value::Null,
        }
        .with_effective_policy_defaults();

        let source = record
            .inherited_policy_sources()
            .into_iter()
            .next()
            .expect("fallback source");
        assert_eq!(source.scope_level, EnterprisePolicyScopeLevel::Resource);
    }

    #[test]
    fn policy_decision_defaults_keep_version_stable_across_dynamic_reason_text() {
        let base = PolicyDecisionRecord {
            decision_id: "decision-dynamic-a".to_string(),
            tenant_context: TenantContext::explicit_user_workspace(
                "acme",
                "finance",
                None,
                "user-finance",
            ),
            requester_context: None,
            actor_id: Some("user-finance".to_string()),
            session_id: Some("session-1".to_string()),
            message_id: Some("message-1".to_string()),
            run_id: Some("run-1".to_string()),
            automation_id: Some("automation-1".to_string()),
            node_id: Some("node-1".to_string()),
            tool: Some("mcp.bank.release_funds".to_string()),
            resource: None,
            data_classes: Vec::new(),
            risk_tier: Some("workflow_phase_tool_scope".to_string()),
            decision: PolicyDecisionEffect::Deny,
            reason_code: "phase_tool_not_allowed".to_string(),
            reason: "tool mcp.bank.release_funds denied for run run-1".to_string(),
            policy_id: Some("workflow_phase_tool_authority".to_string()),
            grant_id: None,
            approval_id: None,
            audit_event_id: None,
            created_at_ms: 1_000,
            metadata: serde_json::json!({
                "phase_tool_authority": {
                    "phase": "publish",
                    "requested_tool": "mcp.bank.release_funds"
                }
            }),
        };
        let first = base.clone().with_effective_policy_defaults();
        let second = PolicyDecisionRecord {
            decision_id: "decision-dynamic-b".to_string(),
            reason: "tool mcp.email.send denied for run run-2".to_string(),
            created_at_ms: 2_000,
            ..base
        }
        .with_effective_policy_defaults();

        assert_eq!(
            first.effective_policy_version_id(),
            second.effective_policy_version_id()
        );
        assert_eq!(
            first.inherited_policy_sources()[0].scope_level,
            EnterprisePolicyScopeLevel::Phase
        );
        assert_eq!(
            first
                .effective_policy_snapshot()
                .expect("snapshot")
                .reason
                .as_str(),
            "tool mcp.bank.release_funds denied for run run-1"
        );
    }
}
