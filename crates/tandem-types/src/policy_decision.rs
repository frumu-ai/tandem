use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tandem_enterprise_contract::{
    DataClass, EffectivePolicySnapshot, EffectivePolicySource, EnterprisePolicyEffect,
    EnterprisePolicyScopeLevel, ResourceRef, TenantContext,
};

pub const EFFECTIVE_POLICY_METADATA_KEY: &str = "effective_policy";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyDecisionEffect {
    Allow,
    Deny,
    ApprovalRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyDecisionRecord {
    pub decision_id: String,
    pub tenant_context: TenantContext,
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
}

impl PolicyDecisionRecord {
    pub fn with_effective_policy_defaults(mut self) -> Self {
        if self.effective_policy_snapshot().is_some() {
            return self;
        }

        let policy_id = self
            .policy_id
            .clone()
            .unwrap_or_else(|| "unspecified_policy".to_string());
        let source = EffectivePolicySource {
            rule_id: policy_id.clone(),
            policy_id,
            version: 1,
            scope_level: EnterprisePolicyScopeLevel::Phase,
            effect: self.decision.enterprise_effect(),
            reason_code: self.reason_code.clone(),
            reason: self.reason.clone(),
            approval_id: self.approval_id.clone(),
        };
        let snapshot = EffectivePolicySnapshot::single_source(
            self.tenant_context.clone(),
            self.created_at_ms,
            source.clone(),
        );
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
        assert_eq!(snapshot.approval_id.as_deref(), Some("approval-1"));
        assert_eq!(
            record.effective_policy_version_id().as_deref(),
            Some(snapshot.policy_version_id.as_str())
        );
        assert_eq!(record.inherited_policy_sources().len(), 1);
    }
}
