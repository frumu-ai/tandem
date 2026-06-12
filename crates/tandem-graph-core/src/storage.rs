use crate::{GraphDomain, GraphScope};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphPartitionKind {
    #[serde(rename = "tenant_project")]
    TenantProject,
    #[serde(rename = "repo_canonical")]
    RepoCanonical,
    #[serde(rename = "repo_worktree")]
    RepoWorktree,
    #[serde(rename = "workflow_version")]
    WorkflowVersion,
    #[serde(rename = "run_ephemeral")]
    RunEphemeral,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphStoragePartition {
    pub kind: GraphPartitionKind,
    pub scope: GraphScope,
    pub domain: GraphDomain,
    pub revision: Option<String>,
    pub retention: GraphRetentionPolicy,
}

impl GraphStoragePartition {
    pub fn canonical_repo(
        scope: GraphScope,
        revision: impl Into<String>,
        retention: GraphRetentionPolicy,
    ) -> Self {
        Self {
            kind: GraphPartitionKind::RepoCanonical,
            scope,
            domain: GraphDomain::Repo,
            revision: Some(revision.into()),
            retention,
        }
    }

    pub fn worktree(
        scope: GraphScope,
        revision: impl Into<String>,
        retention: GraphRetentionPolicy,
    ) -> Self {
        Self {
            kind: GraphPartitionKind::RepoWorktree,
            scope,
            domain: GraphDomain::Repo,
            revision: Some(revision.into()),
            retention,
        }
    }

    pub fn workflow_version(
        scope: GraphScope,
        revision: impl Into<String>,
        retention: GraphRetentionPolicy,
    ) -> Self {
        Self {
            kind: GraphPartitionKind::WorkflowVersion,
            scope,
            domain: GraphDomain::Workflow,
            revision: Some(revision.into()),
            retention,
        }
    }

    pub fn run_ephemeral(scope: GraphScope, retention: GraphRetentionPolicy) -> Self {
        let revision = scope.run_id.clone();
        Self {
            kind: GraphPartitionKind::RunEphemeral,
            scope,
            domain: GraphDomain::Run,
            revision,
            retention,
        }
    }

    pub fn key(&self) -> String {
        [
            self.scope.tenant_id.as_str(),
            self.scope.project_id.as_str(),
            self.scope.workspace_id.as_deref().unwrap_or("_"),
            self.scope.repo_id.as_deref().unwrap_or("_"),
            self.scope.worktree_id.as_deref().unwrap_or("_"),
            self.scope.run_id.as_deref().unwrap_or("_"),
            self.kind.stable_id(),
            self.revision.as_deref().unwrap_or("_"),
        ]
        .join(":")
    }

    pub fn requires_explicit_promotion(&self) -> bool {
        matches!(
            self.kind,
            GraphPartitionKind::RepoWorktree | GraphPartitionKind::RunEphemeral
        )
    }

    pub fn is_visible_to(&self, scope: &GraphScope) -> bool {
        self.scope.tenant_id == scope.tenant_id
            && self.scope.project_id == scope.project_id
            && scoped_id_matches(&self.scope.workspace_id, &scope.workspace_id)
            && scoped_id_matches(&self.scope.repo_id, &scope.repo_id)
            && scoped_id_matches(&self.scope.worktree_id, &scope.worktree_id)
            && scoped_id_matches(&self.scope.run_id, &scope.run_id)
    }
}

fn scoped_id_matches(partition_id: &Option<String>, caller_id: &Option<String>) -> bool {
    partition_id
        .as_ref()
        .is_none_or(|partition_id| caller_id.as_ref() == Some(partition_id))
}

impl GraphPartitionKind {
    pub fn stable_id(&self) -> &'static str {
        match self {
            Self::TenantProject => "tenant_project",
            Self::RepoCanonical => "repo_canonical",
            Self::RepoWorktree => "repo_worktree",
            Self::WorkflowVersion => "workflow_version",
            Self::RunEphemeral => "run_ephemeral",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphRetentionPolicy {
    pub class: GraphRetentionClass,
    pub ttl_ms: Option<u64>,
    pub delete_on_project_delete: bool,
    pub delete_on_workspace_delete: bool,
    pub compact_history_after_ms: Option<u64>,
}

impl GraphRetentionPolicy {
    pub fn durable_project() -> Self {
        Self {
            class: GraphRetentionClass::Durable,
            ttl_ms: None,
            delete_on_project_delete: true,
            delete_on_workspace_delete: true,
            compact_history_after_ms: None,
        }
    }

    pub fn ephemeral_run(ttl_ms: u64) -> Self {
        Self {
            class: GraphRetentionClass::Ephemeral,
            ttl_ms: Some(ttl_ms),
            delete_on_project_delete: true,
            delete_on_workspace_delete: true,
            compact_history_after_ms: None,
        }
    }

    pub fn audit_retained(compact_history_after_ms: u64) -> Self {
        Self {
            class: GraphRetentionClass::AuditRetained,
            ttl_ms: None,
            delete_on_project_delete: true,
            delete_on_workspace_delete: true,
            compact_history_after_ms: Some(compact_history_after_ms),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphRetentionClass {
    #[serde(rename = "durable")]
    Durable,
    #[serde(rename = "ephemeral")]
    Ephemeral,
    #[serde(rename = "audit_retained")]
    AuditRetained,
}
