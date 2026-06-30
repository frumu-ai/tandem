use crate::{
    types::{GovernedReadEvidence, GovernedReadTarget},
    GovernedMemoryTier, MemoryPartition, MemoryTrustLabel, PromotionReview,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_enterprise_contract::{DataClass, ResourceRef};

pub const KNOWLEDGE_SCOPE_METADATA_KEY: &str = "knowledge_scope_registry";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeScopePolicy {
    pub registry_id: String,
    pub resource_ref: ResourceRef,
    pub data_class: DataClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collection_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_binding_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_object_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_org_unit_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_workflow_phases: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_write_tiers: Vec<GovernedMemoryTier>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_promotion_tiers: Vec<GovernedMemoryTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_trust_label: Option<MemoryTrustLabel>,
    #[serde(default)]
    pub promotion_requires_approval: bool,
}

impl KnowledgeScopePolicy {
    pub fn from_metadata(metadata: Option<&Value>) -> Result<Option<Self>, String> {
        let Some(metadata) = metadata else {
            return Ok(None);
        };
        let Some(value) = metadata
            .get(KNOWLEDGE_SCOPE_METADATA_KEY)
            .or_else(|| metadata.get("knowledge_scope"))
        else {
            return Ok(None);
        };
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| format!("invalid_knowledge_scope_registry:{error}"))
    }

    pub fn metadata_value(&self) -> Value {
        json!({ KNOWLEDGE_SCOPE_METADATA_KEY: self })
    }

    pub fn governed_read_target(&self) -> GovernedReadTarget {
        GovernedReadTarget {
            resource_ref: self.resource_ref.clone(),
            data_class: self.data_class,
            source_binding_id: self.source_binding_id.clone(),
            source_object_id: self.source_object_id.clone(),
            evidence: GovernedReadEvidence::SourceBinding,
        }
    }

    pub fn read_denial_reason(&self, workflow_phase: Option<&str>, now_ms: u64) -> Option<String> {
        if self.is_expired_at(now_ms) {
            return Some("knowledge_scope_retention_expired".to_string());
        }
        if self.allowed_workflow_phases.is_empty() {
            return None;
        }
        let Some(workflow_phase) = workflow_phase else {
            return Some("knowledge_scope_missing_workflow_phase".to_string());
        };
        if self
            .allowed_workflow_phases
            .iter()
            .any(|allowed| allowed == "*" || allowed == workflow_phase)
        {
            None
        } else {
            Some("knowledge_scope_phase_denied".to_string())
        }
    }

    pub fn write_decision(
        &self,
        partition: &MemoryPartition,
        now_ms: u64,
    ) -> KnowledgeScopeDecision {
        if let Some(reason) = self.partition_denial_reason(partition) {
            return KnowledgeScopeDecision::deny(reason);
        }
        if self.is_expired_at(now_ms) {
            return KnowledgeScopeDecision::deny("knowledge_scope_retention_expired");
        }
        if !self.allowed_write_tiers.contains(&partition.tier) {
            return KnowledgeScopeDecision::deny("knowledge_write_tier_denied_by_scope");
        }
        KnowledgeScopeDecision::allow("knowledge_write_scope_allowed")
    }

    pub fn promotion_decision(
        &self,
        partition: &MemoryPartition,
        to_tier: GovernedMemoryTier,
        review: &PromotionReview,
        now_ms: u64,
    ) -> KnowledgeScopeDecision {
        if let Some(reason) = self.partition_denial_reason(partition) {
            return KnowledgeScopeDecision::deny(reason);
        }
        if self.is_expired_at(now_ms) {
            return KnowledgeScopeDecision::deny("knowledge_scope_retention_expired");
        }
        if !self.allowed_promotion_tiers.contains(&to_tier) {
            return KnowledgeScopeDecision::deny("knowledge_promotion_tier_denied_by_scope");
        }
        if self.promotion_requires_approval
            && (review.approval_id.is_none() || review.reviewer_id.is_none())
        {
            return KnowledgeScopeDecision::deny("knowledge_promotion_approval_required");
        }
        KnowledgeScopeDecision::allow("knowledge_promotion_scope_allowed")
    }

    fn is_expired_at(&self, now_ms: u64) -> bool {
        self.retention_expires_at_ms
            .is_some_and(|expires_at_ms| expires_at_ms <= now_ms)
    }

    fn partition_denial_reason(&self, partition: &MemoryPartition) -> Option<&'static str> {
        if self.resource_ref.organization_id != partition.org_id
            || self.resource_ref.workspace_id != partition.workspace_id
        {
            return Some("knowledge_scope_tenant_mismatch");
        }
        if self.resource_ref.project_id.as_deref() != Some(partition.project_id.as_str()) {
            return Some("knowledge_scope_project_mismatch");
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeScopeDecision {
    pub allowed: bool,
    pub reason_code: String,
}

impl KnowledgeScopeDecision {
    pub fn allow(reason_code: impl Into<String>) -> Self {
        Self {
            allowed: true,
            reason_code: reason_code.into(),
        }
    }

    pub fn deny(reason_code: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason_code: reason_code.into(),
        }
    }
}

pub fn memory_write_scope_decision(
    partition: &MemoryPartition,
    metadata: Option<&Value>,
    now_ms: u64,
) -> Result<KnowledgeScopeDecision, String> {
    let Some(policy) = KnowledgeScopePolicy::from_metadata(metadata)? else {
        return Ok(KnowledgeScopeDecision::allow(
            "legacy_memory_write_without_knowledge_scope",
        ));
    };
    Ok(policy.write_decision(partition, now_ms))
}

pub fn memory_promotion_scope_decision(
    partition: &MemoryPartition,
    to_tier: GovernedMemoryTier,
    review: &PromotionReview,
    source_metadata: Option<&Value>,
    now_ms: u64,
) -> Result<KnowledgeScopeDecision, String> {
    let Some(policy) = KnowledgeScopePolicy::from_metadata(source_metadata)? else {
        return Ok(KnowledgeScopeDecision::allow(
            "legacy_memory_promotion_without_knowledge_scope",
        ));
    };
    Ok(policy.promotion_decision(partition, to_tier, review, now_ms))
}

pub fn metadata_has_knowledge_scope(metadata: Option<&Value>) -> bool {
    metadata
        .map(|metadata| {
            metadata.get(KNOWLEDGE_SCOPE_METADATA_KEY).is_some()
                || metadata.get("knowledge_scope").is_some()
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_enterprise_contract::ResourceKind;

    fn policy() -> KnowledgeScopePolicy {
        KnowledgeScopePolicy {
            registry_id: "registry-acme-project".to_string(),
            resource_ref: ResourceRef::new(
                "org-a",
                "ws-a",
                ResourceKind::KnowledgeSpace,
                "space-a",
            )
            .with_project_id("project-a"),
            data_class: DataClass::Confidential,
            collection_id: Some("collection-a".to_string()),
            source_binding_id: Some("binding-a".to_string()),
            source_object_id: Some("source-a".to_string()),
            owner_org_unit_id: Some("ou-a".to_string()),
            risk_tier: Some("confidential".to_string()),
            allowed_workflow_phases: vec!["research".to_string()],
            allowed_write_tiers: vec![GovernedMemoryTier::Session],
            allowed_promotion_tiers: vec![GovernedMemoryTier::Project],
            retention_expires_at_ms: Some(2_000),
            required_trust_label: Some(MemoryTrustLabel::HumanApproved),
            promotion_requires_approval: true,
        }
    }

    fn partition(tier: GovernedMemoryTier) -> MemoryPartition {
        MemoryPartition {
            org_id: "org-a".to_string(),
            workspace_id: "ws-a".to_string(),
            project_id: "project-a".to_string(),
            tier,
        }
    }

    #[test]
    fn scoped_write_defaults_to_session_unless_policy_allows_wider_tier() {
        let metadata = policy().metadata_value();

        let session = memory_write_scope_decision(
            &partition(GovernedMemoryTier::Session),
            Some(&metadata),
            1_000,
        )
        .expect("session decision");
        assert!(session.allowed);

        let project = memory_write_scope_decision(
            &partition(GovernedMemoryTier::Project),
            Some(&metadata),
            1_000,
        )
        .expect("project decision");
        assert!(!project.allowed);
        assert_eq!(project.reason_code, "knowledge_write_tier_denied_by_scope");
    }

    #[test]
    fn promotion_across_scope_requires_approval_and_unexpired_policy() {
        let metadata = policy().metadata_value();
        let missing_review = PromotionReview {
            required: true,
            reviewer_id: None,
            approval_id: None,
        };
        let denied = memory_promotion_scope_decision(
            &partition(GovernedMemoryTier::Session),
            GovernedMemoryTier::Project,
            &missing_review,
            Some(&metadata),
            1_000,
        )
        .expect("promotion decision");
        assert!(!denied.allowed);
        assert_eq!(denied.reason_code, "knowledge_promotion_approval_required");

        let approved_review = PromotionReview {
            required: true,
            reviewer_id: Some("reviewer-a".to_string()),
            approval_id: Some("approval-a".to_string()),
        };
        let allowed = memory_promotion_scope_decision(
            &partition(GovernedMemoryTier::Session),
            GovernedMemoryTier::Project,
            &approved_review,
            Some(&metadata),
            1_000,
        )
        .expect("approved promotion decision");
        assert!(allowed.allowed);

        let expired = memory_promotion_scope_decision(
            &partition(GovernedMemoryTier::Session),
            GovernedMemoryTier::Project,
            &approved_review,
            Some(&metadata),
            2_000,
        )
        .expect("expired promotion decision");
        assert!(!expired.allowed);
        assert_eq!(expired.reason_code, "knowledge_scope_retention_expired");
    }
}
