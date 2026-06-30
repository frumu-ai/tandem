use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::DataClass;

/// Governance-facing tier model for scoped memory access.
///
/// Note: `team` and `curated` are included for policy/capability contracts
/// before storage-layer migrations complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedMemoryTier {
    Session,
    Project,
    Team,
    Curated,
}

impl std::fmt::Display for GovernedMemoryTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Session => write!(f, "session"),
            Self::Project => write!(f, "project"),
            Self::Team => write!(f, "team"),
            Self::Curated => write!(f, "curated"),
        }
    }
}

/// Hard partition for memory operations in corporate/LAN environments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryPartition {
    pub org_id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub tier: GovernedMemoryTier,
}

impl MemoryPartition {
    pub fn key(&self) -> String {
        format!(
            "{}/{}/{}/{}",
            self.org_id, self.workspace_id, self.project_id, self.tier
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryClassification {
    Internal,
    Restricted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryAuthorityOperation {
    Read,
    Write,
    Promote,
    Export,
    Decrypt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryAuthorityJobContext {
    pub org_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    pub project_id: String,
    pub actor_id: String,
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_binding_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_class: Option<DataClass>,
    pub classification: MemoryClassification,
    pub operation: MemoryAuthorityOperation,
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_decision_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAuthorityJobContextError {
    Missing,
    TenantMismatch,
    ProjectMismatch,
    ActorMissing,
    ActorMismatch,
    RunMismatch,
    PurposeMissing,
    OperationMismatch,
    ClassificationMismatch,
    SourceMemoryMismatch,
}

impl MemoryAuthorityJobContextError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "memory authority job context missing",
            Self::TenantMismatch => "memory authority job tenant mismatch",
            Self::ProjectMismatch => "memory authority job project mismatch",
            Self::ActorMissing => "memory authority job actor missing",
            Self::ActorMismatch => "memory authority job actor mismatch",
            Self::RunMismatch => "memory authority job run mismatch",
            Self::PurposeMissing => "memory authority job purpose missing",
            Self::OperationMismatch => "memory authority job operation mismatch",
            Self::ClassificationMismatch => "memory authority job classification mismatch",
            Self::SourceMemoryMismatch => "memory authority job source memory mismatch",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MemoryAuthorityJobValidation<'a> {
    pub context: Option<&'a MemoryAuthorityJobContext>,
    pub require_context: bool,
    pub org_id: &'a str,
    pub workspace_id: &'a str,
    pub deployment_id: Option<&'a str>,
    pub actor_id: Option<&'a str>,
    pub run_id: &'a str,
    pub partition: &'a MemoryPartition,
    pub operation: MemoryAuthorityOperation,
    pub classification: Option<MemoryClassification>,
    pub source_memory_id: Option<&'a str>,
}

pub fn validate_memory_authority_job_context(
    validation: MemoryAuthorityJobValidation<'_>,
) -> Result<(), MemoryAuthorityJobContextError> {
    let MemoryAuthorityJobValidation {
        context,
        require_context,
        org_id,
        workspace_id,
        deployment_id,
        actor_id,
        run_id,
        partition,
        operation,
        classification,
        source_memory_id,
    } = validation;
    let Some(context) = context else {
        return if require_context {
            Err(MemoryAuthorityJobContextError::Missing)
        } else {
            Ok(())
        };
    };

    if context.org_id != org_id
        || context.workspace_id != workspace_id
        || context.org_id != partition.org_id
        || context.workspace_id != partition.workspace_id
        || context.deployment_id.as_deref() != deployment_id
    {
        return Err(MemoryAuthorityJobContextError::TenantMismatch);
    }
    if context.project_id != partition.project_id {
        return Err(MemoryAuthorityJobContextError::ProjectMismatch);
    }
    if context.actor_id.trim().is_empty() {
        return Err(MemoryAuthorityJobContextError::ActorMissing);
    }
    if actor_id.is_some_and(|expected| context.actor_id != expected) {
        return Err(MemoryAuthorityJobContextError::ActorMismatch);
    }
    if context.run_id != run_id {
        return Err(MemoryAuthorityJobContextError::RunMismatch);
    }
    if context.purpose.trim().is_empty() {
        return Err(MemoryAuthorityJobContextError::PurposeMissing);
    }
    if context.operation != operation {
        return Err(MemoryAuthorityJobContextError::OperationMismatch);
    }
    if classification.is_some_and(|expected| context.classification != expected) {
        return Err(MemoryAuthorityJobContextError::ClassificationMismatch);
    }
    if let Some(source_memory_id) = source_memory_id {
        if !context
            .source_memory_ids
            .iter()
            .any(|candidate| candidate == source_memory_id)
        {
            return Err(MemoryAuthorityJobContextError::SourceMemoryMismatch);
        }
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryCapabilities {
    #[serde(default)]
    pub read_tiers: Vec<GovernedMemoryTier>,
    #[serde(default)]
    pub write_tiers: Vec<GovernedMemoryTier>,
    #[serde(default)]
    pub promote_targets: Vec<GovernedMemoryTier>,
    #[serde(default = "default_require_review_for_promote")]
    pub require_review_for_promote: bool,
    #[serde(default)]
    pub allow_auto_use_tiers: Vec<GovernedMemoryTier>,
}

fn default_require_review_for_promote() -> bool {
    true
}

impl Default for MemoryCapabilities {
    fn default() -> Self {
        Self {
            read_tiers: vec![GovernedMemoryTier::Session, GovernedMemoryTier::Project],
            write_tiers: vec![GovernedMemoryTier::Session],
            promote_targets: Vec::new(),
            require_review_for_promote: true,
            allow_auto_use_tiers: vec![GovernedMemoryTier::Curated],
        }
    }
}

/// Run-scoped capability token claims for memory access.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryCapabilityToken {
    pub run_id: String,
    pub subject: String,
    pub org_id: String,
    pub workspace_id: String,
    pub project_id: String,
    pub memory: MemoryCapabilities,
    pub expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MemoryRetrievalBudgets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_queries_per_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_top_k: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_results_per_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens_per_window: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_chars_per_window: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRetrievalGrant {
    pub grant_id: String,
    pub subject: String,
    pub org_id: String,
    pub workspace_id: String,
    #[serde(default)]
    pub project_ids: Vec<String>,
    #[serde(default)]
    pub source_binding_ids: Vec<String>,
    #[serde(default)]
    pub source_object_ids: Vec<String>,
    #[serde(default)]
    pub data_classes: Vec<DataClass>,
    #[serde(default)]
    pub budgets: MemoryRetrievalBudgets,
    #[serde(default)]
    pub revoked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRetrievalGatewayRequest {
    pub grant: MemoryRetrievalGrant,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryRetrievalBudgetWindow {
    pub started_at_ms: u64,
    pub query_count: u32,
    pub result_count: u32,
    pub token_count: i64,
    pub char_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryContentKind {
    SolutionCapsule,
    Note,
    Fact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTrustLabel {
    ExternalUserSupplied,
    ConnectorSourced,
    Verified,
    HumanApproved,
    SystemGenerated,
}

impl MemoryTrustLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExternalUserSupplied => "external_user_supplied",
            Self::ConnectorSourced => "connector_sourced",
            Self::Verified => "verified",
            Self::HumanApproved => "human_approved",
            Self::SystemGenerated => "system_generated",
        }
    }

    pub fn is_trusted_for_promotion(self) -> bool {
        matches!(
            self,
            Self::Verified | Self::HumanApproved | Self::SystemGenerated
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryPutRequest {
    pub run_id: String,
    pub partition: MemoryPartition,
    pub kind: MemoryContentKind,
    pub content: String,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    pub classification: MemoryClassification,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_job_context: Option<MemoryAuthorityJobContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryPutResponse {
    pub id: String,
    pub stored: bool,
    pub tier: GovernedMemoryTier,
    pub partition_key: String,
    pub audit_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionReview {
    pub required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PromotionSourceOutcome {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approved: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryPromoteRequest {
    pub run_id: String,
    pub source_memory_id: String,
    pub from_tier: GovernedMemoryTier,
    pub to_tier: GovernedMemoryTier,
    pub partition: MemoryPartition,
    pub reason: String,
    pub review: PromotionReview,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_job_context: Option<MemoryAuthorityJobContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_outcome: Option<PromotionSourceOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrubStatus {
    Passed,
    Redacted,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScrubReport {
    pub status: ScrubStatus,
    pub redactions: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryPromoteResponse {
    pub promoted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_memory_id: Option<String>,
    pub to_tier: GovernedMemoryTier,
    pub scrub_report: ScrubReport,
    pub audit_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_decision_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemorySearchRequest {
    pub run_id: String,
    pub query: String,
    #[serde(default)]
    pub read_scopes: Vec<GovernedMemoryTier>,
    pub partition: MemoryPartition,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_job_context: Option<MemoryAuthorityJobContext>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_gateway: Option<MemoryRetrievalGatewayRequest>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        alias = "workflowPhase",
        alias = "workflow_phase"
    )]
    pub workflow_phase: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemorySearchResponse {
    #[serde(default)]
    pub results: Vec<serde_json::Value>,
    #[serde(default)]
    pub scopes_used: Vec<GovernedMemoryTier>,
    #[serde(default)]
    pub blocked_scopes: Vec<GovernedMemoryTier>,
    pub audit_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_capabilities_are_fail_safe() {
        let caps = MemoryCapabilities::default();
        assert_eq!(
            caps.read_tiers,
            vec![GovernedMemoryTier::Session, GovernedMemoryTier::Project]
        );
        assert_eq!(caps.write_tiers, vec![GovernedMemoryTier::Session]);
        assert!(caps.promote_targets.is_empty());
        assert!(caps.require_review_for_promote);
        assert_eq!(caps.allow_auto_use_tiers, vec![GovernedMemoryTier::Curated]);
    }

    #[test]
    fn partition_key_is_stable() {
        let partition = MemoryPartition {
            org_id: "org_acme".to_string(),
            workspace_id: "ws_tandem".to_string(),
            project_id: "proj_engine".to_string(),
            tier: GovernedMemoryTier::Project,
        };
        assert_eq!(
            partition.key(),
            "org_acme/ws_tandem/proj_engine/project".to_string()
        );
    }

    fn authority_context() -> (MemoryPartition, MemoryAuthorityJobContext) {
        let partition = MemoryPartition {
            org_id: "org-1".to_string(),
            workspace_id: "ws-1".to_string(),
            project_id: "proj-1".to_string(),
            tier: GovernedMemoryTier::Project,
        };
        let context = MemoryAuthorityJobContext {
            org_id: partition.org_id.clone(),
            workspace_id: partition.workspace_id.clone(),
            deployment_id: Some("deploy-1".to_string()),
            project_id: partition.project_id.clone(),
            actor_id: "reviewer-1".to_string(),
            run_id: "run-1".to_string(),
            node_id: Some("node-1".to_string()),
            task_id: Some("task-1".to_string()),
            purpose: "promote approved memory".to_string(),
            source_binding_id: Some("workflow:wf-1".to_string()),
            data_class: Some(DataClass::Internal),
            classification: MemoryClassification::Internal,
            operation: MemoryAuthorityOperation::Promote,
            source_memory_ids: vec!["mem-1".to_string()],
            artifact_refs: vec!["artifact://run-1/task-1".to_string()],
            policy_decision_id: Some("policy-1".to_string()),
            grant_decision_id: Some("grant-1".to_string()),
        };
        (partition, context)
    }

    #[test]
    fn authority_context_revalidates_execution_scope() {
        let (partition, context) = authority_context();

        let result = validate_memory_authority_job_context(MemoryAuthorityJobValidation {
            context: Some(&context),
            require_context: true,
            org_id: "org-1",
            workspace_id: "ws-1",
            deployment_id: Some("deploy-1"),
            actor_id: Some("reviewer-1"),
            run_id: "run-1",
            partition: &partition,
            operation: MemoryAuthorityOperation::Promote,
            classification: Some(MemoryClassification::Internal),
            source_memory_id: Some("mem-1"),
        });

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn authority_context_fails_closed_for_tampered_scope() {
        let (partition, mut context) = authority_context();
        context.operation = MemoryAuthorityOperation::Write;

        let result = validate_memory_authority_job_context(MemoryAuthorityJobValidation {
            context: Some(&context),
            require_context: true,
            org_id: "org-1",
            workspace_id: "ws-1",
            deployment_id: Some("deploy-1"),
            actor_id: Some("reviewer-1"),
            run_id: "run-1",
            partition: &partition,
            operation: MemoryAuthorityOperation::Promote,
            classification: Some(MemoryClassification::Internal),
            source_memory_id: Some("mem-1"),
        });

        assert_eq!(
            result,
            Err(MemoryAuthorityJobContextError::OperationMismatch)
        );
        assert_eq!(
            MemoryAuthorityJobContextError::OperationMismatch.as_str(),
            "memory authority job operation mismatch"
        );
    }

    #[test]
    fn missing_authority_context_can_be_required() {
        let (partition, _) = authority_context();

        let result = validate_memory_authority_job_context(MemoryAuthorityJobValidation {
            context: None,
            require_context: true,
            org_id: "org-1",
            workspace_id: "ws-1",
            deployment_id: Some("deploy-1"),
            actor_id: Some("reviewer-1"),
            run_id: "run-1",
            partition: &partition,
            operation: MemoryAuthorityOperation::Promote,
            classification: None,
            source_memory_id: None,
        });

        assert_eq!(result, Err(MemoryAuthorityJobContextError::Missing));
    }
}
