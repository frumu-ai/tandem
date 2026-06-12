use crate::GraphScope;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provenance {
    #[serde(rename = "extracted")]
    Extracted,
    #[serde(rename = "configured")]
    Configured,
    #[serde(rename = "observed")]
    Observed,
    #[serde(rename = "inferred")]
    Inferred,
    #[serde(rename = "summarized")]
    Summarized,
    #[serde(rename = "ambiguous")]
    Ambiguous,
}

impl Provenance {
    pub fn is_source_truth(&self) -> bool {
        matches!(self, Self::Extracted | Self::Configured | Self::Observed)
    }

    pub fn requires_source_confirmation(&self) -> bool {
        !self.is_source_truth()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreshnessSource {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "commit")]
    Commit,
    #[serde(rename = "index_revision")]
    IndexRevision,
    #[serde(rename = "workflow_version")]
    WorkflowVersion,
    #[serde(rename = "run")]
    Run,
    #[serde(rename = "memory_snapshot")]
    MemorySnapshot,
    #[serde(rename = "policy_hash")]
    PolicyHash,
    #[serde(rename = "tool_schema_hash")]
    ToolSchemaHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Freshness {
    pub source: FreshnessSource,
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked_at_unix_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_after_unix_ms: Option<u64>,
}

impl Freshness {
    pub fn unknown() -> Self {
        Self {
            source: FreshnessSource::Unknown,
            revision: None,
            checked_at_unix_ms: None,
            stale_after_unix_ms: None,
        }
    }

    pub fn from_revision(source: FreshnessSource, revision: impl Into<String>) -> Self {
        Self {
            source,
            revision: Some(revision.into()),
            checked_at_unix_ms: None,
            stale_after_unix_ms: None,
        }
    }

    pub fn with_checked_at(mut self, checked_at_unix_ms: u64) -> Self {
        self.checked_at_unix_ms = Some(checked_at_unix_ms);
        self
    }

    pub fn with_stale_after(mut self, stale_after_unix_ms: u64) -> Self {
        self.stale_after_unix_ms = Some(stale_after_unix_ms);
        self
    }

    pub fn is_unknown(&self) -> bool {
        self.source == FreshnessSource::Unknown || self.revision.is_none()
    }

    pub fn is_stale_at(&self, now_unix_ms: u64) -> bool {
        self.stale_after_unix_ms
            .is_some_and(|stale_after| now_unix_ms >= stale_after)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Visibility {
    pub tenant_id: Option<String>,
    pub project_id: Option<String>,
    pub run_id: Option<String>,
    pub readable_paths: Vec<String>,
    pub redacted: bool,
}

impl Visibility {
    pub fn for_scope(scope: &GraphScope) -> Self {
        Self {
            tenant_id: Some(scope.tenant_id.clone()),
            project_id: Some(scope.project_id.clone()),
            run_id: scope.run_id.clone(),
            readable_paths: Vec::new(),
            redacted: false,
        }
    }

    pub fn redacted(mut self) -> Self {
        self.redacted = true;
        self
    }

    pub fn with_readable_paths(
        mut self,
        paths: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        self.readable_paths = paths.into_iter().map(Into::into).collect();
        self
    }

    pub fn allows_scope(&self, scope: &GraphScope) -> bool {
        self.tenant_id.as_ref() == Some(&scope.tenant_id)
            && self.project_id.as_ref() == Some(&scope.project_id)
            && self
                .run_id
                .as_ref()
                .is_none_or(|run_id| scope.run_id.as_ref() == Some(run_id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    #[serde(rename = "allowed")]
    Allowed,
    #[serde(rename = "denied")]
    Denied { reason: String },
    #[serde(rename = "requires_approval")]
    RequiresApproval { approval_gate: String },
}

impl PolicyDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed)
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied { .. })
    }
}
