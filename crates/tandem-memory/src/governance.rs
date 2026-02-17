use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryContentKind {
    SolutionCapsule,
    Note,
    Fact,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryPromoteRequest {
    pub run_id: String,
    pub source_memory_id: String,
    pub from_tier: GovernedMemoryTier,
    pub to_tier: GovernedMemoryTier,
    pub partition: MemoryPartition,
    pub reason: String,
    pub review: PromotionReview,
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
}
