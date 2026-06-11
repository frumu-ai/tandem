use serde::{Deserialize, Serialize};

pub const CURRENT_GRAPH_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSchemaVersion(pub u32);

impl Default for GraphSchemaVersion {
    fn default() -> Self {
        Self(CURRENT_GRAPH_SCHEMA_VERSION)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphScope {
    pub tenant_id: String,
    pub project_id: String,
    pub workspace_id: Option<String>,
    pub repo_id: Option<String>,
    pub worktree_id: Option<String>,
    pub run_id: Option<String>,
}

impl GraphScope {
    pub fn new(tenant_id: impl Into<String>, project_id: impl Into<String>) -> Self {
        Self {
            tenant_id: tenant_id.into(),
            project_id: project_id.into(),
            workspace_id: None,
            repo_id: None,
            worktree_id: None,
            run_id: None,
        }
    }

    pub fn with_repo(mut self, repo_id: impl Into<String>) -> Self {
        self.repo_id = Some(repo_id.into());
        self
    }

    pub fn with_run(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeId {
    pub schema_version: GraphSchemaVersion,
    pub scope: GraphScope,
    pub kind: String,
    pub key: String,
}

impl NodeId {
    pub fn new(scope: GraphScope, kind: impl Into<String>, key: impl Into<String>) -> Self {
        Self {
            schema_version: GraphSchemaVersion::default(),
            scope,
            kind: kind.into(),
            key: key.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EdgeId {
    pub schema_version: GraphSchemaVersion,
    pub scope: GraphScope,
    pub kind: String,
    pub source_key: String,
    pub target_key: String,
    pub fact_hash: String,
}

impl EdgeId {
    pub fn new(
        scope: GraphScope,
        kind: impl Into<String>,
        source_key: impl Into<String>,
        target_key: impl Into<String>,
        fact_hash: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: GraphSchemaVersion::default(),
            scope,
            kind: kind.into(),
            source_key: source_key.into(),
            target_key: target_key.into(),
            fact_hash: fact_hash.into(),
        }
    }
}
