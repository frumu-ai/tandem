use crate::{GraphPayload, GraphScope};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphAuditEvent {
    pub event_type: GraphAuditEventType,
    pub scope: GraphScope,
    pub actor_id: String,
    pub run_id: Option<String>,
    pub target: GraphAuditTarget,
    pub decision: GraphAuditDecision,
    pub metrics: GraphAuditMetrics,
    pub safe_details: GraphPayload,
}

impl GraphAuditEvent {
    pub fn new(
        event_type: GraphAuditEventType,
        scope: GraphScope,
        actor_id: impl Into<String>,
        target: GraphAuditTarget,
    ) -> Self {
        let run_id = scope.run_id.clone();
        Self {
            event_type,
            scope,
            actor_id: actor_id.into(),
            run_id,
            target,
            decision: GraphAuditDecision::Allowed,
            metrics: GraphAuditMetrics::default(),
            safe_details: GraphPayload::new(),
        }
    }

    pub fn denied(mut self, reason: impl Into<String>) -> Self {
        self.decision = GraphAuditDecision::Denied {
            reason: reason.into(),
        };
        self
    }

    pub fn with_metric_counts(
        mut self,
        nodes: u64,
        edges: u64,
        denied: u64,
        duration_ms: u64,
    ) -> Self {
        self.metrics.nodes = nodes;
        self.metrics.edges = edges;
        self.metrics.denied = denied;
        self.metrics.duration_ms = duration_ms;
        self
    }

    pub fn with_safe_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.safe_details.insert(key.into(), value.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphAuditEventType {
    #[serde(rename = "graph.index.started")]
    IndexStarted,
    #[serde(rename = "graph.index.completed")]
    IndexCompleted,
    #[serde(rename = "graph.index.failed")]
    IndexFailed,
    #[serde(rename = "graph.query.started")]
    QueryStarted,
    #[serde(rename = "graph.query.completed")]
    QueryCompleted,
    #[serde(rename = "graph.query.denied")]
    QueryDenied,
    #[serde(rename = "graph.context_bundle.created")]
    ContextBundleCreated,
    #[serde(rename = "graph.policy.filtered")]
    PolicyFiltered,
    #[serde(rename = "graph.index.stale_fallback")]
    StaleIndexFallback,
    #[serde(rename = "graph.dirty_nodes.invalidated")]
    DirtyNodesInvalidated,
}

impl GraphAuditEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::IndexStarted => "graph.index.started",
            Self::IndexCompleted => "graph.index.completed",
            Self::IndexFailed => "graph.index.failed",
            Self::QueryStarted => "graph.query.started",
            Self::QueryCompleted => "graph.query.completed",
            Self::QueryDenied => "graph.query.denied",
            Self::ContextBundleCreated => "graph.context_bundle.created",
            Self::PolicyFiltered => "graph.policy.filtered",
            Self::StaleIndexFallback => "graph.index.stale_fallback",
            Self::DirtyNodesInvalidated => "graph.dirty_nodes.invalidated",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphAuditTarget {
    pub partition_key: Option<String>,
    pub tool_name: Option<String>,
    pub query_kind: Option<String>,
    pub artifact_ref: Option<String>,
}

impl GraphAuditTarget {
    pub fn query(tool_name: impl Into<String>, query_kind: impl Into<String>) -> Self {
        Self {
            partition_key: None,
            tool_name: Some(tool_name.into()),
            query_kind: Some(query_kind.into()),
            artifact_ref: None,
        }
    }

    pub fn partition(partition_key: impl Into<String>) -> Self {
        Self {
            partition_key: Some(partition_key.into()),
            tool_name: None,
            query_kind: None,
            artifact_ref: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphAuditDecision {
    #[serde(rename = "allowed")]
    Allowed,
    #[serde(rename = "denied")]
    Denied { reason: String },
    #[serde(rename = "fallback")]
    Fallback { reason: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphAuditMetrics {
    pub nodes: u64,
    pub edges: u64,
    pub denied: u64,
    pub duration_ms: u64,
    pub token_savings_estimate: Option<u64>,
    pub cache_hit: Option<bool>,
}
