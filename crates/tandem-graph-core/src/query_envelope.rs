use crate::GraphScope;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphQueryEnvelope {
    pub scope: GraphScope,
    pub actor_id: String,
    pub automation_id: Option<String>,
    pub run_id: Option<String>,
    pub readable_paths: Vec<String>,
    pub writable_paths: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub allowed_memory_tiers: Vec<String>,
    pub budget_tokens: Option<u64>,
    pub approvals: Vec<String>,
    pub context_assertion: Option<String>,
}

impl GraphQueryEnvelope {
    pub fn new(scope: GraphScope, actor_id: impl Into<String>) -> Self {
        Self {
            scope,
            actor_id: actor_id.into(),
            automation_id: None,
            run_id: None,
            readable_paths: Vec::new(),
            writable_paths: Vec::new(),
            allowed_tools: Vec::new(),
            allowed_memory_tiers: Vec::new(),
            budget_tokens: None,
            approvals: Vec::new(),
            context_assertion: None,
        }
    }

    pub fn validate(&self) -> Result<(), GraphQueryEnvelopeError> {
        let mut missing = Vec::new();
        if self.scope.tenant_id.trim().is_empty() {
            missing.push("tenant_id".to_string());
        }
        if self.scope.project_id.trim().is_empty() {
            missing.push("project_id".to_string());
        }
        if self.actor_id.trim().is_empty() {
            missing.push("actor_id".to_string());
        }
        if self.readable_paths.is_empty() {
            missing.push("readable_paths".to_string());
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(GraphQueryEnvelopeError { missing })
        }
    }

    pub fn allows_tool(&self, tool: &str) -> bool {
        self.allowed_tools.iter().any(|allowed| allowed == tool)
    }

    pub fn allows_path(&self, path: &str) -> bool {
        self.readable_paths
            .iter()
            .any(|scope| path_matches_scope(path, scope))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphQueryEnvelopeError {
    pub missing: Vec<String>,
}

impl std::fmt::Display for GraphQueryEnvelopeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "graph query envelope is missing required fields: {}",
            self.missing.join(", ")
        )
    }
}

impl std::error::Error for GraphQueryEnvelopeError {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphQueryAudit {
    pub denied_count: usize,
    pub denied_reasons: Vec<String>,
}

impl GraphQueryAudit {
    pub fn deny(&mut self, reason: impl Into<String>) {
        self.denied_count += 1;
        let reason = reason.into();
        if !self.denied_reasons.contains(&reason) {
            self.denied_reasons.push(reason);
        }
    }

    pub fn allowed(&self) -> bool {
        self.denied_count == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphQueryOutput<T> {
    pub value: T,
    pub audit: GraphQueryAudit,
}

impl<T> GraphQueryOutput<T> {
    pub fn new(value: T, audit: GraphQueryAudit) -> Self {
        Self { value, audit }
    }
}

fn path_matches_scope(path: &str, scope: &str) -> bool {
    let scope = scope.trim_matches('/');
    if scope.is_empty() || scope == "." {
        return true;
    }
    let path = path.trim_matches('/');
    path == scope
        || path
            .strip_prefix(scope)
            .is_some_and(|rest| rest.starts_with('/'))
}
