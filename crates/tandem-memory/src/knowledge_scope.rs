use crate::{
    types::{GovernedReadEvidence, GovernedReadTarget},
    GovernedMemoryTier, MemoryAuthorityJobContext, MemoryPartition, MemoryTrustLabel,
    PromotionReview,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_enterprise_contract::{DataClass, ResourceKind, ResourceRef};

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
            // Knowledge-scoped records are grant-governed: org-unit access flows
            // through projected org-unit grants, not the membership check that
            // applies to ordinary tenant-local memory.
            owner_org_unit_id: None,
            owner_subject: None,
            // Grant-governed (SourceBinding evidence), so the department
            // fail-closed default does not apply here.
            tenant_shared: false,
        }
    }

    pub fn read_denial_reason(&self, workflow_phase: Option<&str>, now_ms: u64) -> Option<String> {
        if self.is_expired_at(now_ms) {
            return Some("knowledge_scope_retention_expired".to_string());
        }
        if self.allowed_workflow_phases.is_empty() {
            return None;
        }
        if self
            .allowed_workflow_phases
            .iter()
            .any(|allowed| allowed == "*")
        {
            return None;
        }
        let Some(workflow_phase) = workflow_phase else {
            return Some("knowledge_scope_missing_workflow_phase".to_string());
        };
        if self
            .allowed_workflow_phases
            .iter()
            .any(|allowed| allowed == workflow_phase)
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
    memory_write_scope_decision_for_context(partition, metadata, None, now_ms)
}

pub fn memory_write_scope_decision_for_context(
    partition: &MemoryPartition,
    metadata: Option<&Value>,
    authority_job_context: Option<&MemoryAuthorityJobContext>,
    now_ms: u64,
) -> Result<KnowledgeScopeDecision, String> {
    memory_write_scope_decision_for_context_with_enterprise_mode(
        partition,
        metadata,
        authority_job_context,
        false,
        now_ms,
    )
}

pub fn memory_write_scope_decision_for_context_with_enterprise_mode(
    partition: &MemoryPartition,
    metadata: Option<&Value>,
    authority_job_context: Option<&MemoryAuthorityJobContext>,
    require_scope_metadata: bool,
    now_ms: u64,
) -> Result<KnowledgeScopeDecision, String> {
    let Some(policy) = KnowledgeScopePolicy::from_metadata(metadata)? else {
        if source_bound_scope_required(metadata, authority_job_context, require_scope_metadata) {
            return Ok(KnowledgeScopeDecision::deny(
                "knowledge_scope_required_for_source_bound_memory_write",
            ));
        }
        return Ok(KnowledgeScopeDecision::allow(
            "legacy_memory_write_without_knowledge_scope",
        ));
    };
    if let Some(reason) =
        source_bound_policy_denial_reason(&policy, metadata, authority_job_context)
    {
        return Ok(KnowledgeScopeDecision::deny(reason));
    }
    Ok(policy.write_decision(partition, now_ms))
}

pub fn memory_promotion_scope_decision(
    partition: &MemoryPartition,
    to_tier: GovernedMemoryTier,
    review: &PromotionReview,
    source_metadata: Option<&Value>,
    now_ms: u64,
) -> Result<KnowledgeScopeDecision, String> {
    memory_promotion_scope_decision_for_context(
        partition,
        to_tier,
        review,
        source_metadata,
        None,
        now_ms,
    )
}

pub fn memory_promotion_scope_decision_for_context(
    partition: &MemoryPartition,
    to_tier: GovernedMemoryTier,
    review: &PromotionReview,
    source_metadata: Option<&Value>,
    authority_job_context: Option<&MemoryAuthorityJobContext>,
    now_ms: u64,
) -> Result<KnowledgeScopeDecision, String> {
    memory_promotion_scope_decision_for_context_with_enterprise_mode(
        partition,
        to_tier,
        review,
        source_metadata,
        authority_job_context,
        false,
        now_ms,
    )
}

pub fn memory_promotion_scope_decision_for_context_with_enterprise_mode(
    partition: &MemoryPartition,
    to_tier: GovernedMemoryTier,
    review: &PromotionReview,
    source_metadata: Option<&Value>,
    authority_job_context: Option<&MemoryAuthorityJobContext>,
    require_scope_metadata: bool,
    now_ms: u64,
) -> Result<KnowledgeScopeDecision, String> {
    let Some(policy) = KnowledgeScopePolicy::from_metadata(source_metadata)? else {
        if source_bound_scope_required(
            source_metadata,
            authority_job_context,
            require_scope_metadata,
        ) {
            return Ok(KnowledgeScopeDecision::deny(
                "knowledge_scope_required_for_source_bound_memory_promotion",
            ));
        }
        return Ok(KnowledgeScopeDecision::allow(
            "legacy_memory_promotion_without_knowledge_scope",
        ));
    };
    if let Some(reason) =
        source_bound_policy_denial_reason(&policy, source_metadata, authority_job_context)
    {
        return Ok(KnowledgeScopeDecision::deny(reason));
    }
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

pub fn metadata_has_enterprise_source_binding(metadata: Option<&Value>) -> bool {
    metadata
        .and_then(|metadata| metadata.get("enterprise_source_binding"))
        .is_some()
}

pub fn source_bound_authority_context(
    authority_job_context: Option<&MemoryAuthorityJobContext>,
) -> bool {
    authority_job_context
        .and_then(|context| context.source_binding_id.as_deref())
        .map(str::trim)
        .is_some_and(|source_binding_id| !source_binding_id.is_empty())
}

fn source_bound_scope_required(
    metadata: Option<&Value>,
    authority_job_context: Option<&MemoryAuthorityJobContext>,
    require_scope_metadata: bool,
) -> bool {
    metadata_has_enterprise_source_binding(metadata)
        || source_bound_authority_context(authority_job_context)
        || require_scope_metadata
}

fn source_bound_policy_denial_reason(
    policy: &KnowledgeScopePolicy,
    metadata: Option<&Value>,
    authority_job_context: Option<&MemoryAuthorityJobContext>,
) -> Option<&'static str> {
    let has_source_binding_metadata = metadata_has_enterprise_source_binding(metadata);
    if let Some(reason) = source_bound_metadata_policy_denial_reason(policy, metadata) {
        return Some(reason);
    }
    source_bound_authority_policy_denial_reason(
        policy,
        authority_job_context,
        !has_source_binding_metadata,
    )
}

fn source_bound_metadata_policy_denial_reason(
    policy: &KnowledgeScopePolicy,
    metadata: Option<&Value>,
) -> Option<&'static str> {
    let binding = metadata?.get("enterprise_source_binding")?;
    let source_binding_id = binding
        .get("binding_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let Some(source_binding_id) = source_binding_id else {
        return Some("knowledge_scope_source_binding_mismatch");
    };
    let data_class = match binding.get("data_class") {
        Some(value) => match serde_json::from_value(value.clone()) {
            Ok(data_class) => Some(data_class),
            Err(_) => return Some("knowledge_scope_data_class_mismatch"),
        },
        None => return Some("knowledge_scope_data_class_mismatch"),
    };
    let source_resource_ref = match binding.get("resource_ref") {
        Some(value) => match serde_json::from_value(value.clone()) {
            Ok(resource_ref) => resource_ref,
            Err(_) => return Some("knowledge_scope_source_resource_mismatch"),
        },
        None => return Some("knowledge_scope_source_resource_mismatch"),
    };
    if !source_bound_resource_ref_matches(&policy.resource_ref, &source_resource_ref) {
        return Some("knowledge_scope_source_resource_mismatch");
    }
    source_bound_policy_claim_denial_reason(policy, Some(source_binding_id), data_class)
}

fn source_bound_authority_policy_denial_reason(
    policy: &KnowledgeScopePolicy,
    authority_job_context: Option<&MemoryAuthorityJobContext>,
    require_authority_resource_ref: bool,
) -> Option<&'static str> {
    let context = authority_job_context?;
    let source_binding_id = context
        .source_binding_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let data_class = match context.data_class {
        Some(data_class) => Some(data_class),
        None if require_authority_resource_ref => {
            return Some("knowledge_scope_data_class_mismatch");
        }
        None => None,
    };
    if let Some(reason) =
        source_bound_policy_claim_denial_reason(policy, Some(source_binding_id), data_class)
    {
        return Some(reason);
    }
    if require_authority_resource_ref
        && (policy.resource_ref.resource_kind != ResourceKind::SourceBinding
            || policy.resource_ref.resource_id != source_binding_id)
    {
        return Some("knowledge_scope_source_resource_mismatch");
    }
    None
}

fn source_bound_policy_claim_denial_reason(
    policy: &KnowledgeScopePolicy,
    source_binding_id: Option<&str>,
    data_class: Option<DataClass>,
) -> Option<&'static str> {
    if let Some(source_binding_id) = source_binding_id {
        let policy_source_binding_id = policy
            .source_binding_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if policy_source_binding_id != Some(source_binding_id) {
            return Some("knowledge_scope_source_binding_mismatch");
        }
        if policy.resource_ref.resource_kind == ResourceKind::SourceBinding
            && policy.resource_ref.resource_id != source_binding_id
        {
            return Some("knowledge_scope_source_binding_mismatch");
        }
    }
    if data_class.is_some_and(|data_class| policy.data_class != data_class) {
        return Some("knowledge_scope_data_class_mismatch");
    }
    None
}

fn source_bound_resource_ref_matches(
    policy_resource_ref: &ResourceRef,
    source_resource_ref: &ResourceRef,
) -> bool {
    policy_resource_ref.organization_id == source_resource_ref.organization_id
        && policy_resource_ref.workspace_id == source_resource_ref.workspace_id
        && policy_resource_ref.resource_kind == source_resource_ref.resource_kind
        && policy_resource_ref.resource_id == source_resource_ref.resource_id
        && source_resource_ref
            .project_id
            .as_ref()
            .is_none_or(|project_id| policy_resource_ref.project_id.as_ref() == Some(project_id))
        && policy_resource_ref.parent_path == source_resource_ref.parent_path
        && policy_resource_ref.branch_id == source_resource_ref.branch_id
        && policy_resource_ref.path_prefix == source_resource_ref.path_prefix
}

pub fn knowledge_scope_policy_from_authority_job_context(
    partition: &MemoryPartition,
    authority_job_context: &MemoryAuthorityJobContext,
    registry_id: impl Into<String>,
    allowed_write_tiers: Vec<GovernedMemoryTier>,
    allowed_promotion_tiers: Vec<GovernedMemoryTier>,
    promotion_requires_approval: bool,
) -> Option<KnowledgeScopePolicy> {
    let source_binding_id = authority_job_context
        .source_binding_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let data_class = authority_job_context.data_class?;
    Some(KnowledgeScopePolicy {
        registry_id: registry_id.into(),
        resource_ref: ResourceRef::new(
            &partition.org_id,
            &partition.workspace_id,
            ResourceKind::SourceBinding,
            source_binding_id,
        )
        .with_project_id(&partition.project_id),
        data_class,
        collection_id: None,
        source_binding_id: Some(source_binding_id.to_string()),
        source_object_id: None,
        owner_org_unit_id: None,
        risk_tier: None,
        allowed_workflow_phases: Vec::new(),
        allowed_write_tiers,
        allowed_promotion_tiers,
        retention_expires_at_ms: None,
        required_trust_label: None,
        promotion_requires_approval,
    })
}

pub fn metadata_with_knowledge_scope(
    metadata: Option<Value>,
    policy: &KnowledgeScopePolicy,
) -> Option<Value> {
    let mut metadata = match metadata {
        Some(Value::Object(map)) => Value::Object(map),
        Some(value) => json!({ "legacy_metadata": value }),
        None => json!({}),
    };
    if let Value::Object(map) = &mut metadata {
        map.insert(
            KNOWLEDGE_SCOPE_METADATA_KEY.to_string(),
            serde_json::to_value(policy).unwrap_or(Value::Null),
        );
    }
    Some(metadata)
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

    fn authority_context(operation: crate::MemoryAuthorityOperation) -> MemoryAuthorityJobContext {
        MemoryAuthorityJobContext {
            org_id: "org-a".to_string(),
            workspace_id: "ws-a".to_string(),
            deployment_id: None,
            project_id: "project-a".to_string(),
            actor_id: "reviewer-a".to_string(),
            run_id: "run-a".to_string(),
            node_id: Some("draft".to_string()),
            task_id: Some("task-a".to_string()),
            purpose: "approved source-bound memory".to_string(),
            source_binding_id: Some("binding-a".to_string()),
            data_class: Some(DataClass::Confidential),
            classification: crate::MemoryClassification::Internal,
            operation,
            source_memory_ids: vec!["memory-a".to_string()],
            artifact_refs: Vec::new(),
            policy_decision_id: Some("approval-a".to_string()),
            grant_decision_id: Some("approval-a".to_string()),
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
    fn enterprise_write_without_scope_metadata_is_denied() {
        let decision = memory_write_scope_decision_for_context_with_enterprise_mode(
            &partition(GovernedMemoryTier::Session),
            None,
            None,
            true,
            1_000,
        )
        .expect("write decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_required_for_source_bound_memory_write"
        );
    }

    #[test]
    fn tenant_scoped_legacy_write_without_enterprise_mode_remains_allowed() {
        let decision =
            memory_write_scope_decision(&partition(GovernedMemoryTier::Session), None, 1_000)
                .expect("write decision");

        assert!(decision.allowed);
        assert_eq!(
            decision.reason_code,
            "legacy_memory_write_without_knowledge_scope"
        );
    }

    #[test]
    fn local_legacy_write_without_scope_metadata_remains_allowed() {
        let decision = memory_write_scope_decision(
            &MemoryPartition {
                org_id: "local".to_string(),
                workspace_id: "local".to_string(),
                project_id: "local".to_string(),
                tier: GovernedMemoryTier::Session,
            },
            None,
            1_000,
        )
        .expect("write decision");

        assert!(decision.allowed);
        assert_eq!(
            decision.reason_code,
            "legacy_memory_write_without_knowledge_scope"
        );
    }

    #[test]
    fn wildcard_workflow_phase_does_not_require_concrete_phase() {
        let mut policy = policy();
        policy.allowed_workflow_phases = vec!["*".to_string()];

        assert_eq!(policy.read_denial_reason(None, 1_000), None);
        assert_eq!(policy.read_denial_reason(Some("review"), 1_000), None);
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

    #[test]
    fn source_bound_authority_write_requires_knowledge_scope_metadata() {
        let decision = memory_write_scope_decision_for_context(
            &partition(GovernedMemoryTier::Session),
            None,
            Some(&authority_context(crate::MemoryAuthorityOperation::Write)),
            1_000,
        )
        .expect("write decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_required_for_source_bound_memory_write"
        );
    }

    #[test]
    fn enterprise_authority_write_without_source_binding_requires_knowledge_scope_metadata() {
        let mut context = authority_context(crate::MemoryAuthorityOperation::Write);
        context.source_binding_id = None;
        let decision = memory_write_scope_decision_for_context_with_enterprise_mode(
            &partition(GovernedMemoryTier::Session),
            None,
            Some(&context),
            true,
            1_000,
        )
        .expect("write decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_required_for_source_bound_memory_write"
        );
    }

    #[test]
    fn source_bound_authority_promotion_requires_knowledge_scope_metadata() {
        let review = PromotionReview {
            required: true,
            reviewer_id: Some("reviewer-a".to_string()),
            approval_id: Some("approval-a".to_string()),
        };
        let decision = memory_promotion_scope_decision_for_context(
            &partition(GovernedMemoryTier::Session),
            GovernedMemoryTier::Project,
            &review,
            None,
            Some(&authority_context(crate::MemoryAuthorityOperation::Promote)),
            1_000,
        )
        .expect("promotion decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_required_for_source_bound_memory_promotion"
        );
    }

    #[test]
    fn enterprise_promotion_without_scope_metadata_is_denied() {
        let review = PromotionReview {
            required: true,
            reviewer_id: Some("reviewer-a".to_string()),
            approval_id: Some("approval-a".to_string()),
        };
        let decision = memory_promotion_scope_decision_for_context_with_enterprise_mode(
            &partition(GovernedMemoryTier::Session),
            GovernedMemoryTier::Project,
            &review,
            None,
            None,
            true,
            1_000,
        )
        .expect("promotion decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_required_for_source_bound_memory_promotion"
        );
    }

    #[test]
    fn tenant_scoped_legacy_promotion_without_enterprise_mode_remains_allowed() {
        let review = PromotionReview {
            required: true,
            reviewer_id: Some("reviewer-a".to_string()),
            approval_id: Some("approval-a".to_string()),
        };
        let decision = memory_promotion_scope_decision(
            &partition(GovernedMemoryTier::Session),
            GovernedMemoryTier::Project,
            &review,
            None,
            1_000,
        )
        .expect("promotion decision");

        assert!(decision.allowed);
        assert_eq!(
            decision.reason_code,
            "legacy_memory_promotion_without_knowledge_scope"
        );
    }

    #[test]
    fn enterprise_authority_promotion_without_source_binding_requires_knowledge_scope_metadata() {
        let review = PromotionReview {
            required: true,
            reviewer_id: Some("reviewer-a".to_string()),
            approval_id: Some("approval-a".to_string()),
        };
        let mut context = authority_context(crate::MemoryAuthorityOperation::Promote);
        context.source_binding_id = None;
        let decision = memory_promotion_scope_decision_for_context_with_enterprise_mode(
            &partition(GovernedMemoryTier::Session),
            GovernedMemoryTier::Project,
            &review,
            None,
            Some(&context),
            true,
            1_000,
        )
        .expect("promotion decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_required_for_source_bound_memory_promotion"
        );
    }

    #[test]
    fn authority_context_can_stamp_explicit_knowledge_scope_policy() {
        let partition = partition(GovernedMemoryTier::Session);
        let context = authority_context(crate::MemoryAuthorityOperation::Write);
        let policy = knowledge_scope_policy_from_authority_job_context(
            &partition,
            &context,
            "registry-authority",
            vec![GovernedMemoryTier::Session],
            vec![GovernedMemoryTier::Project],
            true,
        )
        .expect("policy");
        let metadata = metadata_with_knowledge_scope(None, &policy);

        let decision = memory_write_scope_decision_for_context(
            &partition,
            metadata.as_ref(),
            Some(&context),
            1_000,
        )
        .expect("write decision");

        assert!(decision.allowed);
        assert_eq!(decision.reason_code, "knowledge_write_scope_allowed");
    }

    #[test]
    fn source_bound_authority_write_rejects_mismatched_knowledge_scope_binding() {
        let mut policy = policy();
        policy.source_binding_id = Some("binding-b".to_string());
        let metadata = policy.metadata_value();
        let decision = memory_write_scope_decision_for_context(
            &partition(GovernedMemoryTier::Session),
            Some(&metadata),
            Some(&authority_context(crate::MemoryAuthorityOperation::Write)),
            1_000,
        )
        .expect("write decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_source_binding_mismatch"
        );
    }

    #[test]
    fn source_bound_authority_write_rejects_wrong_knowledge_scope_resource_kind() {
        let policy = policy();
        let metadata = policy.metadata_value();
        let decision = memory_write_scope_decision_for_context(
            &partition(GovernedMemoryTier::Session),
            Some(&metadata),
            Some(&authority_context(crate::MemoryAuthorityOperation::Write)),
            1_000,
        )
        .expect("write decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_source_resource_mismatch"
        );
    }

    #[test]
    fn source_bound_authority_write_rejects_missing_authority_data_class() {
        let partition = partition(GovernedMemoryTier::Session);
        let mut context = authority_context(crate::MemoryAuthorityOperation::Write);
        let policy = knowledge_scope_policy_from_authority_job_context(
            &partition,
            &context,
            "registry-authority",
            vec![GovernedMemoryTier::Session],
            vec![GovernedMemoryTier::Project],
            true,
        )
        .expect("policy");
        context.data_class = None;
        let metadata = metadata_with_knowledge_scope(None, &policy);
        let decision = memory_write_scope_decision_for_context(
            &partition,
            metadata.as_ref(),
            Some(&context),
            1_000,
        )
        .expect("write decision");

        assert!(!decision.allowed);
        assert_eq!(decision.reason_code, "knowledge_scope_data_class_mismatch");
    }

    #[test]
    fn source_bound_authority_promotion_rejects_mismatched_knowledge_scope_data_class() {
        let mut policy = policy();
        policy.data_class = DataClass::Internal;
        let metadata = policy.metadata_value();
        let review = PromotionReview {
            required: true,
            reviewer_id: Some("reviewer-a".to_string()),
            approval_id: Some("approval-a".to_string()),
        };
        let decision = memory_promotion_scope_decision_for_context(
            &partition(GovernedMemoryTier::Session),
            GovernedMemoryTier::Project,
            &review,
            Some(&metadata),
            Some(&authority_context(crate::MemoryAuthorityOperation::Promote)),
            1_000,
        )
        .expect("promotion decision");

        assert!(!decision.allowed);
        assert_eq!(decision.reason_code, "knowledge_scope_data_class_mismatch");
    }

    #[test]
    fn source_bound_metadata_write_rejects_mismatched_knowledge_scope_binding() {
        let mut policy = policy();
        policy.resource_ref =
            ResourceRef::new("org-a", "ws-a", ResourceKind::SourceBinding, "binding-b")
                .with_project_id("project-a");
        let mut metadata = policy.metadata_value();
        metadata.as_object_mut().expect("metadata object").insert(
            "enterprise_source_binding".to_string(),
            json!({
                "binding_id": "binding-b",
                "connector_id": "manual-upload",
                "resource_ref": {
                    "organization_id": "org-a",
                    "workspace_id": "ws-a",
                    "project_id": "project-a",
                    "resource_kind": "source_binding",
                    "resource_id": "binding-b"
                },
                "data_class": "confidential",
                "source_object_id": "source-b"
            }),
        );
        let decision = memory_write_scope_decision(
            &partition(GovernedMemoryTier::Session),
            Some(&metadata),
            1_000,
        )
        .expect("write decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_source_binding_mismatch"
        );
    }

    #[test]
    fn source_bound_metadata_write_rejects_mismatched_knowledge_scope_resource() {
        let mut policy = policy();
        policy.resource_ref = ResourceRef::new(
            "org-a",
            "ws-a",
            ResourceKind::DocumentCollection,
            "allowed-drive",
        )
        .with_project_id("project-a");
        let mut metadata = policy.metadata_value();
        metadata.as_object_mut().expect("metadata object").insert(
            "enterprise_source_binding".to_string(),
            json!({
                "binding_id": "binding-a",
                "connector_id": "manual-upload",
                "resource_ref": {
                    "organization_id": "org-a",
                    "workspace_id": "ws-a",
                    "project_id": "project-a",
                    "resource_kind": "document_collection",
                    "resource_id": "restricted-drive"
                },
                "data_class": "confidential",
                "source_object_id": "source-a"
            }),
        );
        let decision = memory_write_scope_decision(
            &partition(GovernedMemoryTier::Session),
            Some(&metadata),
            1_000,
        )
        .expect("write decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_source_resource_mismatch"
        );
    }

    #[test]
    fn source_bound_metadata_promotion_allows_valid_source_resource_with_authority() {
        let mut policy = policy();
        policy.resource_ref = ResourceRef::new(
            "org-a",
            "ws-a",
            ResourceKind::DocumentCollection,
            "restricted-drive",
        )
        .with_project_id("project-a");
        let mut metadata = policy.metadata_value();
        metadata.as_object_mut().expect("metadata object").insert(
            "enterprise_source_binding".to_string(),
            json!({
                "binding_id": "binding-a",
                "connector_id": "manual-upload",
                "resource_ref": {
                    "organization_id": "org-a",
                    "workspace_id": "ws-a",
                    "project_id": "project-a",
                    "resource_kind": "document_collection",
                    "resource_id": "restricted-drive"
                },
                "data_class": "confidential",
                "source_object_id": "source-a"
            }),
        );
        let review = PromotionReview {
            required: true,
            reviewer_id: Some("reviewer-a".to_string()),
            approval_id: Some("approval-a".to_string()),
        };

        let decision = memory_promotion_scope_decision_for_context(
            &partition(GovernedMemoryTier::Session),
            GovernedMemoryTier::Project,
            &review,
            Some(&metadata),
            Some(&authority_context(crate::MemoryAuthorityOperation::Promote)),
            1_000,
        )
        .expect("promotion decision");

        assert!(decision.allowed);
        assert_eq!(decision.reason_code, "knowledge_promotion_scope_allowed");
    }

    #[test]
    fn source_bound_metadata_write_rejects_missing_source_binding_id() {
        let mut metadata = policy().metadata_value();
        metadata.as_object_mut().expect("metadata object").insert(
            "enterprise_source_binding".to_string(),
            json!({
                "connector_id": "manual-upload",
                "resource_ref": {
                    "organization_id": "org-a",
                    "workspace_id": "ws-a",
                    "project_id": "project-a",
                    "resource_kind": "source_binding",
                    "resource_id": "binding-a"
                },
                "data_class": "confidential",
                "source_object_id": "source-a"
            }),
        );
        let decision = memory_write_scope_decision(
            &partition(GovernedMemoryTier::Session),
            Some(&metadata),
            1_000,
        )
        .expect("write decision");

        assert!(!decision.allowed);
        assert_eq!(
            decision.reason_code,
            "knowledge_scope_source_binding_mismatch"
        );
    }

    #[test]
    fn source_bound_metadata_write_rejects_missing_data_class() {
        let mut metadata = policy().metadata_value();
        metadata.as_object_mut().expect("metadata object").insert(
            "enterprise_source_binding".to_string(),
            json!({
                "binding_id": "binding-a",
                "connector_id": "manual-upload",
                "resource_ref": {
                    "organization_id": "org-a",
                    "workspace_id": "ws-a",
                    "project_id": "project-a",
                    "resource_kind": "source_binding",
                    "resource_id": "binding-a"
                },
                "source_object_id": "source-a"
            }),
        );
        let decision = memory_write_scope_decision(
            &partition(GovernedMemoryTier::Session),
            Some(&metadata),
            1_000,
        )
        .expect("write decision");

        assert!(!decision.allowed);
        assert_eq!(decision.reason_code, "knowledge_scope_data_class_mismatch");
    }
}
